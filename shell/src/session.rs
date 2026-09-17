//! The `qsflow-core` child: newline-delimited JSON over its stdio, plus the
//! JSON-RPC request/response pair the `forget` verb needs.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde::Deserialize;

/// The event loop's inbound side for core traffic.
pub type EventSender = calloop::channel::Sender<Event>;

/// What activating a row does.
///
/// The wire carries one string (`on_click`), which is a documented contract
/// with out-of-process JSON-RPC hosts, so it stays a string on the wire; it is
/// parsed once here, at the boundary, and nothing downstream matches prefixes.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Action {
    /// `launch:<desktop-id>`
    Launch(String),
    /// `run:<shell command>`, and the fallback for a bare command.
    Run(String),
    /// `copy:{"text": …}` — the JSON argument, forwarded as it arrived.
    Copy(String),
    /// `action:<desktop-id>:<action-id>`
    Desktop(String),
    /// A bare URL / `file:` / `mailto:` URI.
    Open(String),
    /// A row that does nothing.
    #[default]
    None,
}

impl Action {
    pub fn parse(on_click: &str) -> Self {
        if on_click.is_empty() {
            return Self::None;
        }
        if let Some(rest) = on_click.strip_prefix("launch:") {
            Self::Launch(rest.to_owned())
        } else if let Some(rest) = on_click.strip_prefix("run:") {
            Self::Run(rest.to_owned())
        } else if let Some(rest) = on_click.strip_prefix("copy:") {
            Self::Copy(rest.to_owned())
        } else if let Some(rest) = on_click.strip_prefix("action:") {
            Self::Desktop(rest.to_owned())
        } else if on_click.starts_with("http")
            || on_click.starts_with("file:")
            || on_click.starts_with("mailto:")
        {
            // The core opens URIs through GLib, which honours the portal and the
            // `.desktop` `Terminal=` key that `xdg-open` drops.
            Self::Open(on_click.to_owned())
        } else {
            Self::Run(on_click.to_owned())
        }
    }

    /// The one line this action turns into; `None` when there is nothing to do.
    pub fn line(&self) -> Option<String> {
        let line = match self {
            Self::Launch(id) => format!("launch {id}\n"),
            Self::Run(command) => format!("run {command}\n"),
            Self::Copy(argument) => format!("copy {argument}\n"),
            Self::Desktop(action) => format!("action {action}\n"),
            Self::Open(uri) => format!("open {uri}\n"),
            Self::None => return None,
        };
        Some(line)
    }
}

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
    /// Filled in from `on_click` as the payload is read.
    #[serde(skip)]
    pub action: Action,
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

    /// Exactly one verb per row, or nothing at all.
    pub fn action(&mut self, action: &Action) {
        if let Some(line) = action.line() {
            self.send(&line);
        }
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
                    if let Ok(mut items) =
                        serde_json::from_value::<Vec<Item>>(value["data"].clone())
                    {
                        let payload = value["data"].to_string();
                        for item in &mut items {
                            item.action = Action::parse(&item.on_click);
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scheme_parses_and_round_trips_to_one_line() {
        for (on_click, line) in [
            ("launch:firefox", "launch firefox\n"),
            ("run:btop --utf", "run btop --utf\n"),
            (r#"copy:{"text":"x"}"#, "copy {\"text\":\"x\"}\n"),
            (
                "action:app.desktop:new-window",
                "action app.desktop:new-window\n",
            ),
            ("https://example.com/a b", "open https://example.com/a b\n"),
            ("file:///tmp/a%20b", "open file:///tmp/a%20b\n"),
            ("mailto:me@example.com", "open mailto:me@example.com\n"),
            // A bare command is still run, not opened.
            ("kitty -e htop", "run kitty -e htop\n"),
        ] {
            assert_eq!(
                Action::parse(on_click).line().as_deref(),
                Some(line),
                "{on_click}"
            );
        }
    }

    #[test]
    fn an_empty_scheme_does_nothing() {
        assert_eq!(Action::parse(""), Action::None);
        assert_eq!(Action::parse("").line(), None);
    }
}
