use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender as StdSender, channel};
use std::sync::{LazyLock, Mutex, PoisonError};

use calloop::channel::Sender;

use crate::session::model::{ResultItem, ThemeConfig};

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Theme(ThemeConfig),
    Results(Vec<ResultItem>),
    /// A JSON-RPC `resolve_icon` reply, matched back to the spec that asked.
    Icon {
        spec: String,
        path: Option<String>,
    },
    /// A JSON-RPC `forget` reply: the row the UI asked to drop, and whether the
    /// core really dropped anything.
    Forgotten {
        on_click: String,
        forgotten: bool,
    },
    /// The core's stdout closed: nothing can be searched or launched anymore.
    CoreExited,
}

/// Lines bound for the core's stdin, drained by one writer thread.
static OUTBOX: LazyLock<Mutex<Option<StdSender<String>>>> = LazyLock::new(|| Mutex::new(None));

/// What an in-flight JSON-RPC request was for, so its reply can be routed.
enum Pending {
    Icon(String),
    Forget(String),
}

static REQUESTS: LazyLock<Mutex<HashMap<u64, Pending>>> = LazyLock::new(Default::default);
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// Send one protocol line to the core (newline added). A no-op before the core
/// exists.
pub fn send(line: &str) {
    let guard = OUTBOX.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(line.to_string());
    }
}

/// Send a JSON-RPC request, remembering what its reply means.
fn request(method: &str, params: serde_json::Value, pending: Pending) -> u64 {
    let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, pending);

    send(
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string(),
    );

    id
}

/// Ask the core to resolve an icon spec to an absolute path; the reply arrives
/// as [`BackendEvent::Icon`]. The returned id only correlates that reply.
pub fn resolve_icon(spec: &str) -> u64 {
    request(
        "resolve_icon",
        serde_json::json!({ "name": spec }),
        Pending::Icon(spec.to_string()),
    )
}

/// Ask the core to forget a row; the reply arrives as
/// [`BackendEvent::Forgotten`] and says whether anything was really dropped.
pub fn forget_row(on_click: &str) {
    request(
        "forget",
        serde_json::json!({ "on_click": on_click }),
        Pending::Forget(on_click.to_string()),
    );
}

/// Spawn the core and wire the reader/writer threads: one writer owns stdin (no
/// interleaved lines), the reader reaps the child when stdout closes.
pub fn start(tx: Sender<BackendEvent>) {
    // Re-exec this same binary in core mode; there is no second file. stderr is
    // inherited so the core's warnings (a host that overran, a plugin that did
    // not answer) land in the unit's journal.
    let spawned = std::env::current_exe().and_then(|exe| {
        Command::new(exe)
            .arg("--core")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
    });

    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("wayrun: cannot start the core: {error}");
            let _ = tx.send(BackendEvent::CoreExited);
            return;
        }
    };

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();

    if let Some(stdin) = stdin {
        let (sender, receiver) = channel::<String>();
        *OUTBOX.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
        std::thread::spawn(move || drain(stdin, receiver));
    }

    if let Some(stdout) = stdout {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(event) = parse(&line)
                    && tx.send(event).is_err()
                {
                    break;
                }
            }
            let _ = tx.send(BackendEvent::CoreExited);
            let _ = child.wait();
        });
    }

    // Warm the core's caches (app list + external host discovery) here rather
    // than on the first keystroke: the daemon boots long before the launcher is
    // shown. A real search supersedes this one and its payload is dropped.
    send("a");
}

fn drain(mut stdin: ChildStdin, receiver: std::sync::mpsc::Receiver<String>) {
    while let Ok(line) = receiver.recv() {
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

    let reply: RpcReply = serde_json::from_str(line).ok()?;
    if reply.jsonrpc.as_deref() != Some("2.0") {
        return None;
    }
    let id = reply.id?;
    let pending = REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id)?;

    match pending {
        Pending::Icon(spec) => {
            let path = reply
                .result
                .as_ref()
                .and_then(|result| result.as_str())
                .map(str::to_string);
            Some(BackendEvent::Icon { spec, path })
        }
        Pending::Forget(on_click) => {
            // a reply with no `forgotten` (or an error) means nothing was
            // dropped, which is the safe answer for the UI
            let forgotten = reply
                .result
                .as_ref()
                .and_then(|result| result.get("forgotten"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            Some(BackendEvent::Forgotten {
                on_click,
                forgotten,
            })
        }
    }
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
    fn present_nulls_and_the_ephemeral_flag_do_not_reject_the_payload() {
        // Help rows carry `on_click: null`, and usage opt-out rows carry
        // `ephemeral: true`; a present `null` must not fail the whole `Vec`.
        let line = r#"{"type":"results","data":[{"title":"Calculator","summary":"* (default)","on_click":null,"icon":null,"ephemeral":true}]}"#;
        match parse(line).unwrap() {
            BackendEvent::Results(items) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].title, "Calculator");
                assert_eq!(items[0].on_click, None);
                assert_eq!(items[0].icon, None);
                assert!(items[0].ephemeral);
            }
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
        REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, Pending::Icon("papirus:folder".to_string()));

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

    #[test]
    fn a_forget_reply_says_whether_anything_was_dropped() {
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, Pending::Forget("run:never-used".to_string()));

        let event = parse(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"forgotten":false}}}}"#
        ))
        .unwrap();
        match event {
            BackendEvent::Forgotten {
                on_click,
                forgotten,
            } => {
                assert_eq!(on_click, "run:never-used");
                assert!(!forgotten);
            }
            other => panic!("expected a forget reply, got {other:?}"),
        }

        // an error reply (or a reply without the field) must not claim a drop
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        REQUESTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, Pending::Forget("run:a".to_string()));
        let event = parse(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32601,"message":"no"}}}}"#
        ))
        .unwrap();
        assert!(matches!(
            event,
            BackendEvent::Forgotten {
                forgotten: false,
                ..
            }
        ));
    }
}
