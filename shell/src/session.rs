//! The `qsflow-core` child: newline-delimited JSON over its stdio, plus the
//! JSON-RPC request/response pair the `forget` verb needs.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde::Deserialize;

/// The event loop's inbound side for core traffic.
pub type EventSender = calloop::channel::Sender<Event>;

/// A row as it comes off the wire; `icon` is an absolute path the core resolved.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Item {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default, rename = "on_click")]
    pub on_click: String,
}

/// The theme fields, still as hex strings: a field the wire omits keeps the
/// QML-era fallback, so an absent value must stay distinguishable from a black.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeData {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub on_primary: Option<String>,
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub fg: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
}

/// Everything the core can tell the shell, delivered on the event loop's channel.
#[derive(Debug)]
pub enum Event {
    Theme(ThemeData),
    /// The rows plus the raw payload, which is what the identical-payload
    /// dedupe compares; a re-send must not reset the cursor.
    Results {
        items: Vec<Item>,
        payload: String,
    },
    /// The answer to a `forget` request; the row leaves the list only when the
    /// core really dropped it.
    Forgotten {
        id: u64,
        forgotten: bool,
    },
    CoreExited,
}

pub struct Session {
    stdin: ChildStdin,
    child: Child,
    next_id: u64,
}

impl Session {
    pub fn spawn(tx: EventSender) -> std::io::Result<Self> {
        let mut child = Command::new("qsflow-core")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        std::thread::spawn(move || read_loop(stdout, tx));
        Ok(Self {
            stdin,
            child,
            next_id: 1,
        })
    }

    pub fn send(&mut self, line: &str) {
        if let Err(err) = self.stdin.write_all(line.as_bytes()) {
            log::warn!("core write failed: {err}");
        }
        let _ = self.stdin.flush();
    }

    /// One line per keystroke; the core debounces and aborts server-side.
    pub fn search(&mut self, text: &str) {
        let mut line = String::with_capacity(text.len() + 1);
        line.push_str(text);
        line.push('\n');
        self.send(&line);
    }

    /// `forget` goes through JSON-RPC because only the answer says whether a row
    /// was really dropped (a built-in provider's forget is a no-op).
    pub fn forget(&mut self, on_click: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let params = serde_json::json!({ "on_click": on_click });
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "forget",
            "params": params,
        });
        self.send(&format!("{request}\n"));
        id
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Closing stdin is what the core's protocol loop watches for.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_loop(stdout: ChildStdout, tx: EventSender) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(kind) = value.get("type").and_then(|v| v.as_str()) {
            match kind {
                "theme" => {
                    if let Ok(data) = serde_json::from_value(value["data"].clone()) {
                        let _ = tx.send(Event::Theme(data));
                    }
                }
                "results" => {
                    if let Ok(items) = serde_json::from_value(value["data"].clone()) {
                        let payload = value["data"].to_string();
                        let _ = tx.send(Event::Results { items, payload });
                    }
                }
                _ => {}
            }
        } else if value.is_array() {
            if let Ok(items) = serde_json::from_value(value.clone()) {
                let _ = tx.send(Event::Results {
                    items,
                    payload: value.to_string(),
                });
            }
        } else if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
            let forgotten = value
                .pointer("/result/forgotten")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let _ = tx.send(Event::Forgotten { id, forgotten });
        }
    }
    let _ = tx.send(Event::CoreExited);
}
