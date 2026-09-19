use std::process::{Command, Stdio};

use calloop::channel::Sender;

/// Read the selection on a worker thread. `generation` is the show it belongs to,
/// so a read finishing after a dismissal is dropped instead of pasted later.
pub fn read(tx: Sender<(u64, Option<String>)>, generation: u64) {
    std::thread::spawn(move || {
        let _ = tx.send((generation, paste()));
    });
}

fn paste() -> Option<String> {
    let output = Command::new("wl-paste")
        .arg("--no-newline")
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}
