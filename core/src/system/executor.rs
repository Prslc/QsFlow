use std::process;
use gio::prelude::*;

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
        .arg(format!("setsid {} >/dev/null 2>&1 &", clean_cmd))
        .spawn()
        .ok();
}

/// Launch an application by desktop id via GLib's `GAppInfo` (`g_app_info_launch`)
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

/// Copy text to the Wayland clipboard via `wl-copy` (no shell involved).
/// The `copy:` scheme carries a JSON object (`{"text":"..."}`) — the same
/// convention as `select`/`forget`; JSON escaping keeps the line protocol
/// safe from embedded newlines and quotes. Silent no-op on parse failure or
/// a missing `wl-copy`.
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
