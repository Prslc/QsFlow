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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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

    /// The one message this action turns into; `None` when there is nothing to
    /// do. Search stays on the text protocol — it is a stream — while the
    /// commands go to the JSON-RPC methods that mirror the text verbs.
    pub fn request(&self) -> Option<String> {
        let (method, params) = match self {
            Self::Launch(id) => ("launch", serde_json::json!({ "desktop_id": id })),
            Self::Run(command) => ("run", serde_json::json!({ "cmd": command })),
            Self::Copy(argument) => (
                "copy",
                match serde_json::from_str(argument) {
                    Ok(value @ serde_json::Value::Object(_)) => value,
                    // Whatever a malformed `copy:` argument held is still text.
                    _ => serde_json::json!({ "text": argument }),
                },
            ),
            Self::Open(uri) => ("open", serde_json::json!({ "uri": uri })),
            Self::Desktop(action) => {
                let (desktop_id, action_id) = action.split_once(':')?;
                (
                    "action",
                    serde_json::json!({ "desktop_id": desktop_id, "action_id": action_id }),
                )
            }
            Self::None => return None,
        };
        Some(format!(
            "{}\n",
            serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params })
        ))
    }
}

/// The wire serialises an optional field the core has no value for as `null`
/// — the `?` help list and a plugin's identity card both carry
/// `on_click: null` — and a present `null` is not a `String`. Without this, one
/// such field rejected the whole `Vec<Item>` and the shell silently kept the
/// previous payload, so help and keyword mode appeared to do nothing.
fn nullable<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// A row as it comes off the wire; `icon` is an absolute path the core resolved.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Item {
    pub title: String,
    #[serde(default, deserialize_with = "nullable")]
    pub summary: String,
    #[serde(default, deserialize_with = "nullable")]
    pub icon: String,
    #[serde(default, rename = "on_click", deserialize_with = "nullable")]
    pub on_click: String,
    /// The core's `ephemeral` flag: a row the plugin does not want recorded in
    /// usage history. Forwarded back in the `select` record.
    #[serde(default)]
    pub ephemeral: bool,
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
            // Inherited so the core's warnings (a host that overran, a plugin
            // that did not answer) land in the unit's journal.
            .stderr(Stdio::inherit())
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

    /// Exactly one request per row, or nothing at all.
    pub fn action(&mut self, action: &Action) {
        if let Some(request) = action.request() {
            self.send(&request);
        }
    }

    /// Records a row's usage through the method that mirrors the text verb.
    pub fn select(&mut self, record: &serde_json::Value) {
        self.send(&format!(
            "{}\n",
            serde_json::json!({ "jsonrpc": "2.0", "method": "select", "params": record })
        ));
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
        } else if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
            let forgotten = value
                .pointer("/result/forgotten")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let _ = tx.send(Event::Forgotten { id, forgotten });
        }
    }
    let _ = tx.send(Event::CoreExited);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_of(on_click: &str) -> Option<serde_json::Value> {
        let line = Action::parse(on_click).request()?;
        Some(serde_json::from_str(line.trim_end()).expect("a request is one JSON object"))
    }

    #[test]
    fn an_ephemeral_row_is_read_off_the_wire() {
        let items: Vec<Item> = serde_json::from_value(serde_json::json!([
            { "title": "repo", "on_click": "https://github.com/x/y", "ephemeral": true },
            { "title": "Firefox", "on_click": "launch:firefox.desktop" },
        ]))
        .unwrap();
        assert!(items[0].ephemeral);
        assert!(!items[1].ephemeral, "an absent flag means record it");
    }

    #[test]
    fn every_scheme_becomes_one_json_rpc_request() {
        for (on_click, method, key, value) in [
            ("launch:firefox", "launch", "desktop_id", "firefox"),
            ("run:btop --utf", "run", "cmd", "btop --utf"),
            (
                "https://example.com/a b",
                "open",
                "uri",
                "https://example.com/a b",
            ),
            ("file:///tmp/a%20b", "open", "uri", "file:///tmp/a%20b"),
            (
                "mailto:me@example.com",
                "open",
                "uri",
                "mailto:me@example.com",
            ),
            // A bare command is still run, not opened.
            ("kitty -e htop", "run", "cmd", "kitty -e htop"),
        ] {
            let request = request_of(on_click).expect(on_click);
            assert_eq!(request["jsonrpc"], "2.0", "{on_click}");
            assert_eq!(request["method"], method, "{on_click}");
            assert_eq!(request["params"][key], value, "{on_click}");
            assert!(
                request.get("id").is_none(),
                "a command needs no reply: {on_click}"
            );
        }

        let request = request_of(r#"copy:{"text":"hi"}"#).expect("copy");
        assert_eq!(request["method"], "copy");
        assert_eq!(request["params"]["text"], "hi");
    }

    #[test]
    fn a_malformed_copy_argument_is_still_text() {
        let request = request_of("copy:not json").expect("copy");
        assert_eq!(request["method"], "copy");
        assert_eq!(request["params"]["text"], "not json");
    }

    #[test]
    fn a_desktop_action_names_its_entry_and_group() {
        let request = request_of("action:org.gnome.Nautilus.desktop:new-window").expect("action");
        assert_eq!(request["method"], "action");
        assert_eq!(
            request["params"]["desktop_id"],
            "org.gnome.Nautilus.desktop"
        );
        assert_eq!(request["params"]["action_id"], "new-window");
    }

    #[test]
    fn an_empty_scheme_does_nothing() {
        assert_eq!(Action::parse(""), Action::None);
        assert_eq!(Action::parse("").request(), None);
    }

    #[test]
    fn a_null_optional_field_does_not_drop_the_payload() {
        // The `?` help list and a plugin's identity card carry `on_click: null`
        // (and a host may leave `summary`/`icon` null): the row must survive as
        // an empty string, not reject the whole `Vec<Item>`.
        let items: Vec<Item> = serde_json::from_value(serde_json::json!([
            {"title": "Youdao Translation", "summary": "Translate text", "on_click": null, "icon": null},
            {"title": "Todo", "summary": null, "icon": "/x.svg"},
        ]))
        .expect("null optional fields are empty, not fatal");
        assert_eq!(items[0].on_click, "");
        assert_eq!(items[0].icon, "");
        assert_eq!(items[0].action, Action::None);
        assert_eq!(items[1].summary, "");
        assert_eq!(items[1].on_click, "");
    }
}
