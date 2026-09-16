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

/// One `(sender, receiver-slot)`: the sender is shared, the receiver is taken
/// exactly once by whoever owns the stream.
type Mailbox<T> = (
    mpsc::UnboundedSender<T>,
    Mutex<Option<mpsc::UnboundedReceiver<T>>>,
);

/// Lines bound for the core's stdin, drained by one writer thread.
static OUTBOX: LazyLock<Mailbox<String>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded();
    (tx, Mutex::new(Some(rx)))
});

/// The core's stdout, handed to the subscription exactly once.
static INBOX: LazyLock<Mailbox<BackendEvent>> = LazyLock::new(|| {
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

/// The line's first byte decides the shape, so the common payloads are parsed
/// once — no `Value` round trip and no `data` clone per keystroke.
fn parse(line: &str) -> Option<BackendEvent> {
    let line = line.trim();
    match line.as_bytes().first()? {
        b'[' => serde_json::from_str::<Vec<ResultItem>>(line)
            .ok()
            .map(BackendEvent::Results),
        b'{' => parse_object(line),
        _ => None,
    }
}

/// `{"type":"theme","data":{…}}` / `{"type":"results","data":[…]}` in one pass.
#[derive(serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
enum Tagged {
    Theme(ThemeConfig),
    Results(Vec<ResultItem>),
}

/// A JSON-RPC reply: only `resolve_icon` answers are expected (matched back by
/// request id).
#[derive(serde::Deserialize)]
struct RpcReply {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<serde_json::Value>,
}

fn parse_object(line: &str) -> Option<BackendEvent> {
    if let Ok(tagged) = serde_json::from_str::<Tagged>(line) {
        return Some(match tagged {
            Tagged::Theme(config) => BackendEvent::Theme(config),
            Tagged::Results(items) => BackendEvent::Results(items),
        });
    }

    // not a theme/results payload: either a watcher row the shell never
    // renders, or a JSON-RPC icon reply
    let reply: RpcReply = serde_json::from_str(line).ok()?;
    if reply.jsonrpc.as_deref() != Some("2.0") {
        return None;
    }
    let id = reply.id?;
    let spec = ICON_REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id)?;
    let path = reply
        .result
        .as_ref()
        .and_then(|result| result.as_str())
        .map(str::to_string);

    Some(BackendEvent::Icon { spec, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_typed_payloads_without_a_value_round_trip() {
        let theme = parse(r##"{"type":"theme","data":{"primary":"#fff"}}"##).unwrap();
        assert!(matches!(theme, BackendEvent::Theme(_)));

        let results = parse(r#"{"type":"results","data":[{"title":"a"}]}"#).unwrap();
        match results {
            BackendEvent::Results(items) => assert_eq!(items[0].title, "a"),
            other => panic!("expected results, got {other:?}"),
        }

        // the bare array form is still accepted
        let bare = parse(r#"[{"title":"b"}]"#).unwrap();
        match bare {
            BackendEvent::Results(items) => assert_eq!(items[0].title, "b"),
            other => panic!("expected results, got {other:?}"),
        }
    }

    #[test]
    fn ignores_lines_the_shell_does_not_render() {
        assert!(parse("").is_none());
        assert!(parse(r#"{"type":"something-else","data":[]}"#).is_none());
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"jsonrpc":"2.0","id":999,"result":"/x.svg"}"#).is_none());
    }

    #[test]
    fn matches_icon_replies_to_their_requested_spec() {
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        ICON_REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, "papirus:folder".to_string());

        let event = parse(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":"/usr/share/icons/x.svg"}}"#
        ))
        .unwrap();
        match event {
            BackendEvent::Icon { spec, path } => {
                assert_eq!(spec, "papirus:folder");
                assert_eq!(path.as_deref(), Some("/usr/share/icons/x.svg"));
            }
            other => panic!("expected icon, got {other:?}"),
        }
    }
}
