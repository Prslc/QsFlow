use std::process::{self, Stdio};

/// Run a shell command detached from the backend (system commands, clipboard
/// paste, translate copy, …). Shell is intended here: `%u`/`%f` leftovers are
/// stripped before execution.
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

/// Launch an application through its `.desktop` file via GLib's `GAppInfo`
/// (`gio launch <path>`). This honours Exec quoting, field codes, env and
/// `DBusActivatable` single-instance semantics — never through a shell. The
/// path is passed as a single argument, so spaces/special chars are safe.
pub fn launch_app(desktop_path: &str) {
    let _ = process::Command::new("gio")
        .arg("launch")
        .arg(desktop_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
