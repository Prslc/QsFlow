//! The `qsflow-core` child process: one long-lived line-protocol connection.
//!
//! The core owns search/usage/launch; this module owns the pipe. Both channels
//! live in `LazyLock`s and each receiving end is taken exactly once, so a
//! subscription rebuild can never start a second core.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};

use futures::channel::mpsc;
use futures::stream::{self, Stream, StreamExt};

use crate::model::{ResultItem, ThemeConfig};

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Theme(ThemeConfig),
    Results(Vec<ResultItem>),
    /// A JSON-RPC `resolve_icon` reply, matched back to the spec that asked.
    Icon {
        spec: String,
        path: Option<String>,
    },
    /// The core's stdout closed: nothing can be searched or launched anymore.
    CoreExited,
}

/// Lines bound for the core's stdin, drained by one writer thread.
static OUTBOX: LazyLock<(
    mpsc::UnboundedSender<String>,
    Mutex<Option<mpsc::UnboundedReceiver<String>>>,
)> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded();
    (tx, Mutex::new(Some(rx)))
});

/// The core's stdout, handed to the subscription exactly once.
static INBOX: LazyLock<(
    mpsc::UnboundedSender<BackendEvent>,
    Mutex<Option<mpsc::UnboundedReceiver<BackendEvent>>>,
)> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded();
    (tx, Mutex::new(Some(rx)))
});

/// Icon spec per in-flight `resolve_icon` request id.
static ICON_REQUESTS: LazyLock<Mutex<HashMap<u64, String>>> = LazyLock::new(Default::default);
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// The core's stdout as a stream. Called once per process by the subscription
/// recipe; a later call finds the receiver taken and yields nothing rather than
/// spawning a second core.
pub fn stream() -> impl Stream<Item = BackendEvent> {
    match take(&INBOX.1) {
        Some(receiver) => {
            start();
            receiver.boxed()
        }
        None => stream::empty().boxed(),
    }
}

/// Send one protocol line to the core (newline added). A no-op before the core
/// exists.
pub fn send(line: &str) {
    let _ = OUTBOX.0.unbounded_send(line.to_string());
}

/// Ask the core to resolve an icon spec (theme name, `papirus:<name>`, …) to an
/// absolute path; the reply arrives as [`BackendEvent::Icon`]. The returned id
/// is only the correlation handle for that reply.
pub fn resolve_icon(spec: &str) -> u64 {
    let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    ICON_REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, spec.to_string());

    send(
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "resolve_icon",
            "params": { "name": spec },
        })
        .to_string(),
    );

    id
}

fn take<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Spawn the core and wire the reader/writer threads. One writer thread owns
/// stdin, so no two producers can interleave half a line; the reader thread
/// reaps the child when stdout closes.
fn start() {
    let spawned = Command::new("qsflow-core")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("qsflow-shell: cannot start qsflow-core: {error}");
            let _ = INBOX.0.unbounded_send(BackendEvent::CoreExited);
            return;
        }
    };

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();

    if let Some(stdin) = stdin
        && let Some(lines) = take(&OUTBOX.1)
    {
        std::thread::spawn(move || drain(stdin, lines));
    }

    if let Some(stdout) = stdout {
        let tx = INBOX.0.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(event) = parse(&line)
                    && tx.unbounded_send(event).is_err()
                {
                    break;
                }
            }
            let _ = tx.unbounded_send(BackendEvent::CoreExited);
            let _ = child.wait();
        });
    }

    // The core builds its per-process application cache on the first app search
    // (~540ms; every later search is 0-1ms), so spend that here instead of on the
    // first keystroke — in resident mode the daemon boots long before the
    // launcher is shown. A real search supersedes this one and only its *await*
    // is aborted: the cache build runs on a blocking task and completes either
    // way. The warmup's own payload is dropped if a search follows it.
    send("a");
}

fn drain(mut stdin: ChildStdin, mut lines: mpsc::UnboundedReceiver<String>) {
    while let Some(line) = futures::executor::block_on(lines.next()) {
        if stdin.write_all(line.as_bytes()).is_err() || stdin.write_all(b"\n").is_err() {
            break;
        }
        let _ = stdin.flush();
    }
}

fn parse(line: &str) -> Option<BackendEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    if let Some(kind) = value.get("type").and_then(|kind| kind.as_str()) {
        let data = value.get("data")?.clone();
        return match kind {
            "theme" => serde_json::from_value(data).ok().map(BackendEvent::Theme),
            "results" => serde_json::from_value(data).ok().map(BackendEvent::Results),
            // the core's watchers re-emit rows the shell never renders
            _ => None,
        };
    }

    if value.is_array() {
        return serde_json::from_value(value)
            .ok()
            .map(BackendEvent::Results);
    }

    if value.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0") {
        let id = value.get("id")?.as_u64()?;
        let spec = ICON_REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id)?;
        let path = value
            .get("result")
            .and_then(|result| result.as_str())
            .map(str::to_string);

        return Some(BackendEvent::Icon { spec, path });
    }

    None
}
