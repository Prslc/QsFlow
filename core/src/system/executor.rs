use std::process;

/// exec command
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