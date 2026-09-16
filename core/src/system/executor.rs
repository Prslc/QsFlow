//! Action execution: shell commands, app/URI launch and clipboard writes.
use gio::prelude::*;
use std::path::Path;
use std::process;

use crate::system::fs;

/// Run a shell command detached (system commands, descriptors that need a
/// shell); `%u`/`%f` leftovers are stripped first.
pub fn execute_command(cmd: &str) {
    let clean_cmd = cmd
        .replace("%u", "")
        .replace("%U", "")
        .replace("%f", "")
        .replace("%F", "");

    process::Command::new("sh")
        .arg("-c")
        .arg(format!("setsid {} >/dev/null 2>&1 &", clean_cmd))
        .spawn()
        .ok();
}

/// Launch an app by desktop id through GLib's `g_app_info_launch`, so Exec
/// quoting, field codes, env and `DBusActivatable` are honoured. A missing id
/// is a silent no-op.
pub fn launch_app(desktop_id: &str) {
    for app in gio::AppInfo::all() {
        if app.id().as_deref() == Some(desktop_id) {
            let _ = app.launch(&[], None::<&gio::AppLaunchContext>);
            return;
        }
    }
}

/// Open a URI with the default handler. Not `Qt.openUrlExternally`: outside a
/// Flatpak/Snap sandbox it falls back to `xdg-open`, which drops `Terminal=true`.
pub fn open_uri(uri: &str) {
    let _ = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>);
}

/// Run the `[Desktop Action <id>]` group of `<desktop-id>`: gio-rs binds no
/// desktop-action launcher, so the `Exec=` line is expanded here and run through
/// the detached shell path. A malformed spec or a missing file/group is a silent
/// no-op.
pub fn launch_desktop_action(spec: &str) {
    let Some((id, action_id)) = spec.split_once(':') else {
        return;
    };
    if id.is_empty() || action_id.is_empty() {
        return;
    }

    let Some(path) = fs::find_desktop_file(id) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Some(exec) = action_exec(&text, action_id) else {
        return;
    };

    execute_command(&expand_exec(&exec, entry_name(&text).as_deref(), &path));
}

/// `Exec=` of the group whose header is exactly `[Desktop Action <id>]`.
fn action_exec(text: &str, action_id: &str) -> Option<String> {
    let header = format!("[Desktop Action {action_id}]");
    let mut in_group = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == header;
        } else if in_group && let Some(exec) = line.strip_prefix("Exec=") {
            let exec = exec.trim();
            if !exec.is_empty() {
                return Some(exec.to_string());
            }
        }
    }
    None
}

/// Unlocalised `Name=` of the `[Desktop Entry]` group.
fn entry_name(text: &str) -> Option<String> {
    let mut in_entry = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Expand the `Exec=` field codes: `%%` `%c` `%k`; `%i` and the file/URL codes
/// are dropped (`execute_command` strips the latter, `%i` is useless in a shell
/// command).
fn expand_exec(exec: &str, name: Option<&str>, desktop: &Path) -> String {
    let mut expanded = String::with_capacity(exec.len());
    let mut chars = exec.chars();

    while let Some(c) = chars.next() {
        if c != '%' {
            expanded.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => expanded.push('%'),
            Some('c') => expanded.push_str(name.unwrap_or("")),
            Some('k') => expanded.push_str(&desktop.to_string_lossy()),
            Some('i' | 'f' | 'F' | 'u' | 'U') => {}
            // a lone trailing `%` is not a field code: keep it verbatim
            None => expanded.push('%'),
            Some(other) => {
                expanded.push('%');
                expanded.push(other);
            }
        }
    }
    expanded
}

/// Write text to the Wayland clipboard via `wl-copy`. The payload is JSON
/// (`{"text":…}`) so embedded newlines/quotes survive the line protocol; a parse
/// failure or a missing `wl-copy` is a silent no-op.
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
    use super::*;

    const DESKTOP: &str = "[Desktop Entry]\n\
         Name=VirtualBox\n\
         Actions=Manager;Home;\n\
         Name[de]=VirtualBox oeffnen\n\
         \n\
         [Desktop Action Manager]\n\
         Name=Open VM Manager\n\
         Exec=VirtualBoxVM --startvm %c %k\n\
         \n\
         [Desktop Action Home]\n\
         Name=Open Home\n\
         Exec=/usr/bin/virtualbox %U %f %i\n";

    #[test]
    fn action_exec_picks_the_named_group() {
        assert_eq!(
            action_exec(DESKTOP, "Manager").as_deref(),
            Some("VirtualBoxVM --startvm %c %k")
        );
        assert_eq!(
            action_exec(DESKTOP, "Home").as_deref(),
            Some("/usr/bin/virtualbox %U %f %i")
        );
        assert_eq!(action_exec(DESKTOP, "Missing"), None);
        // the group prefix must match in full: "Man" is not "Manager"
        assert_eq!(action_exec(DESKTOP, "Man"), None);
    }

    #[test]
    fn entry_name_ignores_action_groups() {
        assert_eq!(entry_name(DESKTOP).as_deref(), Some("VirtualBox"));

        // an action group before [Desktop Entry] must not shadow it
        let reordered = "[Desktop Action Manager]\nName=Open VM Manager\n\n\
                         [Desktop Entry]\nName=Translator\n";
        assert_eq!(entry_name(reordered).as_deref(), Some("Translator"));
    }

    #[test]
    fn expand_exec_substitutes_and_drops_codes() {
        let path = Path::new("/usr/share/applications/virtualbox.desktop");
        assert_eq!(
            expand_exec("run %c -- %k %f %U %i", Some("VirtualBox"), path),
            "run VirtualBox -- /usr/share/applications/virtualbox.desktop   "
        );
        // %% is a literal percent; an absent Name expands to nothing
        assert_eq!(expand_exec("100%% %c", None, path), "100% ");
        // unknown codes stay verbatim
        assert_eq!(expand_exec("%x %", None, path), "%x %");
    }

    #[test]
    fn malformed_specs_are_silent_noops() {
        launch_desktop_action("no-colon");
        launch_desktop_action(":action");
        launch_desktop_action("id:");
        launch_desktop_action("");
    }
}
