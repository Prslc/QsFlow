use gio::prelude::{AppInfoExt, FileExt};
use std::process;

/// Run a shell command detached from the backend (system commands, …). Shell
/// is intended here: `%u`/`%f` leftovers are stripped before execution.
pub fn execute_command(cmd: &str) {
    let clean_cmd = cmd
        .replace("%u", "")
        .replace("%U", "")
        .replace("%f", "")
        .replace("%F", "");

    process::Command::new("sh")
        .arg("-c")
        .arg(format!("setsid {clean_cmd} >/dev/null 2>&1 &"))
        .spawn()
        .ok();
}

/// Join an argv into a `sh` command line, quoting the tokens that need it.
/// The `run:` on_click path is parsed by a shell, so an argv that is safe as
/// argv must be re-quoted before it becomes a command string.
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|token| shell_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quote one token for `sh` only when it contains something the shell would
/// treat specially; a bare word stays readable.
fn shell_quote(token: &str) -> String {
    let safe = !token.is_empty()
        && token.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'=' | b','
                )
        });
    if safe {
        return token.to_string();
    }
    format!("'{}'", token.replace('\'', r"'\''"))
}

/// Run an argv detached from the backend — no shell, because a `.desktop` file's
/// `Exec=` already *is* argv: putting it through `sh -c` would make a `;` or a
/// `$` inside one argument syntax again.
pub fn execute_argv(argv: &[String]) {
    let Some((program, args)) = argv.split_first() else {
        return;
    };

    process::Command::new("setsid")
        .arg(program)
        .args(args)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .spawn()
        .ok();
}

/// Launch an application by desktop id via `GLib`'s `GAppInfo` (`g_app_info_launch`)
/// — no shell, no external `gio` binary. Re-fetches the registered `GAppInfo`
/// so Exec quoting, field codes, env and `DBusActivatable` single-instance are
/// all honoured. Falls back silently (no-op) if the id is not found.
pub fn launch_app(desktop_id: &str) {
    for app in gio::AppInfo::all() {
        if app.id().as_deref() == Some(desktop_id) {
            let _ = app.launch(&[], None::<&gio::AppLaunchContext>);
            return;
        }
    }
}

/// Open a URI with the default handler via `GLib`. `xdg-open` would drop
/// `Terminal=true` outside a Flatpak/Snap sandbox.
pub fn open_uri(uri: &str) {
    let _ = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>);
}

/// Show a file in the file manager. `org.freedesktop.FileManager1.ShowItems`
/// selects the file itself; when no manager implements it, fall back to opening
/// the containing directory (the usual `xdg-open` behaviour).
pub fn reveal(uri: &str) {
    if reveal_via_file_manager(uri) {
        return;
    }
    if let Some(parent) = gio::File::for_uri(uri)
        .path()
        .and_then(|path| path.parent().map(ToOwned::to_owned))
    {
        open_uri(&gio::File::for_path(parent).uri());
    }
}

fn reveal_via_file_manager(uri: &str) -> bool {
    use gio::glib::variant::ToVariant;

    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    let params = ("", vec![uri.to_string()]).to_variant();
    connection
        .call_sync(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1",
            "ShowItems",
            Some(&params),
            None::<&gio::glib::VariantTy>,
            gio::DBusCallFlags::NONE,
            2000,
            None::<&gio::Cancellable>,
        )
        .is_ok()
}

/// Write text to the Wayland clipboard via `wl-copy` (no shell involved).
/// The `copy:` scheme carries JSON (`{"text":…}`) so the line protocol
/// survives embedded newlines/quotes; parse failure or a missing `wl-copy`
/// is a silent no-op.
pub fn copy_json(payload: &str) {
    let Ok(req) = serde_json::from_str::<CopyRequest>(payload) else {
        return;
    };

    let Some(mut child) = process::Command::new("wl-copy")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .spawn()
        .ok()
    else {
        return;
    };
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        let _ = stdin.write_all(req.text.as_bytes());
    }
    drop(child.stdin.take());
    let _ = child.wait();
}

#[derive(serde::Deserialize)]
struct CopyRequest {
    text: String,
}

#[cfg(test)]
mod tests {
    use super::shell_join;

    fn argv(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn safe_tokens_stay_bare() {
        assert_eq!(
            shell_join(&argv(&[
                "niri",
                "msg",
                "action",
                "focus-window",
                "--id",
                "42"
            ])),
            "niri msg action focus-window --id 42"
        );
    }

    #[test]
    fn unsafe_tokens_are_single_quoted() {
        assert_eq!(shell_join(&argv(&["echo", "a b;c"])), "echo 'a b;c'");
        assert_eq!(shell_join(&argv(&["echo", "it's"])), r"echo 'it'\''s'");
    }

    #[test]
    fn an_empty_token_is_quoted_to_survive_the_shell() {
        assert_eq!(shell_join(&argv(&["foo", ""])), "foo ''");
    }
}
