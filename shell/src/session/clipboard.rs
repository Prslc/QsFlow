use std::process::{Command, Stdio};

use calloop::channel::Sender;

/// Read the selection on a worker thread and hand the text to the event loop.
/// `generation` is the show the read was requested for: a read that finishes
/// after a dismissal must be dropped by `on_paste`, not pasted into the next
/// show (a stalled selection owner is exactly why this runs off the loop).
pub fn read(tx: Sender<(u64, Option<String>)>, generation: u64) {
    std::thread::spawn(move || {
        let _ = tx.send((generation, paste()));
    });
}

/// The blocking half, which is why it runs off the event loop.
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
