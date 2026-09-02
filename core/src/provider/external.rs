use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

/// Identity of one plugin as described by an external host's `list_plugins`.
/// The host owns its own name/icon/ready hint; the core just relays them.
#[derive(Clone)]
pub struct HostMeta {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub ready: String,
}

/// A plugin whose results come from an external JSON-RPC subprocess declared
/// in `plugins.toml` via `command`. The core is a generic client: it spawns
/// `command`, relays `search`, and discovers the plugin's identity from the
/// host's `list_plugins` response. It has no compiled-in knowledge of the
/// plugin — the same binary can serve any number of ids.
pub struct External {
    meta: Meta,
    command: String,
}

/// The registry is built once per process, so these small strings live exactly
/// the process lifetime that `Meta`'s `&'static str` requires.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

impl External {
    /// Build from the configured id + host command + host-discovered identity.
    /// `None` identity (host missing or not self-describing) degrades to the
    /// id as display name; search still relays and just yields no results.
    pub fn new(id: &str, command: String, discovered: Option<HostMeta>) -> Self {
        let (name, icon, ready) = match discovered {
            Some(m) => (m.name, m.icon, m.ready),
            None => (
                id.to_string(),
                String::new(),
                format!("External plugin via {command}"),
            ),
        };
        // The host may name its identity with a `papirus:` spec; the UI only
        // renders absolute paths (`file://` + icon), so resolve before the
        // string leaks into `Meta`.
        let icon = if icon.starts_with("papirus:") {
            find_icon_path(&icon).unwrap_or_default()
        } else {
            icon
        };
        Self {
            meta: Meta {
                id: leak(id.to_string()),
                name: leak(name),
                icon: leak(icon),
                ready: leak(ready),
                keyword: leak(id.to_string()),
            },
            command,
        }
    }
}

impl Plugin for External {
    fn meta(&self) -> &Meta {
        &self.meta
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let command = self.command.clone();
        let plugin = self.meta.id.to_string();
        let query = query.to_string();
        let icon = self.meta.icon.to_string();
        Box::pin(async move { query_external(&command, &plugin, &query, &icon).await })
    }

    fn default_view(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<ResultItem>>>> + Send + '_>> {
        let command = self.command.clone();
        let plugin = self.meta.id.to_string();
        let icon = self.meta.icon.to_string();
        Box::pin(async move { query_default(&command, &plugin, &icon).await })
    }
}

/// One JSON-RPC request/response round trip against `command`: spawn, write the
/// request, close stdin, reap, return the first response line (any line that
/// parses). `None` when the host is missing or produced no parseable output.
async fn rpc_call(command: &str, request: &serde_json::Value) -> Option<serde_json::Value> {
    let mut req_str = serde_json::to_string(request).ok()?;
    req_str.push('\n');

    let mut child = Command::new(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?; // command not found -> no host

    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(req_str.as_bytes()).await.is_err() {
            let _ = child.kill().await;
            return None;
        }
        drop(stdin); // close stdin so the host sees EOF (one-shot)
    }

    // `wait_with_output` drains stdout/stderr and reaps the child in one step;
    // reading stdout then `wait()` separately can double-poll the join handle.
    let output = child.wait_with_output().await.ok()?;

    let out = std::str::from_utf8(&output.stdout).unwrap_or_default();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            return Some(v);
        }
    }
    None
}

/// Ask the host who it serves. Returns every plugin it describes via
/// `list_plugins`; empty when the host is missing or does not self-describe.
pub async fn discover(command: &str) -> Vec<HostMeta> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "list_plugins",
        "id": 1,
    });
    let Some(response) = rpc_call(command, &request).await else {
        return Vec::new();
    };
    let Some(list) = response.get("result").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            Some(HostMeta {
                id: entry.get("id")?.as_str()?.to_string(),
                name: entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                icon: entry
                    .get("icon")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                ready: entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Normalize a host response's `result` array into rows, resolving each
/// icon to what the UI can render. `None` when the response has no usable
/// `result` array.
fn parse_result_items(response: &serde_json::Value, icon: &str) -> Option<Vec<ResultItem>> {
    let items = response.get("result")?.as_array()?;
    let mut parsed: Vec<ResultItem> = items
        .iter()
        .filter_map(|it| serde_json::from_value(it.clone()).ok())
        .collect();
    let icon_path = find_icon_path(icon);
    for item in &mut parsed {
        let spec = item.icon.as_deref().unwrap_or("");
        item.icon = resolve_item_icon(spec, icon_path.clone());
    }
    Some(parsed)
}

async fn query_external(
    command: &str,
    plugin: &str,
    text: &str,
    icon: &str,
) -> Result<Vec<ResultItem>> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "search",
        "params": { "plugin": plugin, "text": text },
        "id": 1,
    });
    let Some(response) = rpc_call(command, &request).await else {
        return Ok(Vec::new());
    };
    Ok(parse_result_items(&response, icon).unwrap_or_default())
}

/// Ask the host for its default view (its `top` method). `Ok(None)` when the
/// host has no such method (`-32601`), it errored, or produced no usable
/// result — the caller falls back to the identity card.
async fn query_default(command: &str, plugin: &str, icon: &str) -> Result<Option<Vec<ResultItem>>> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "top",
        "params": { "plugin": plugin },
        "id": 1,
    });
    let Some(response) = rpc_call(command, &request).await else {
        return Ok(None);
    };
    if response.get("error").is_some() {
        return Ok(None);
    }
    Ok(parse_result_items(&response, icon))
}
/// Resolve one result icon to what the UI can render (`file://` + path):
/// empty -> the plugin's own icon, `papirus:` -> absolute Papirus path,
/// anything else (already an absolute path) passes through.
fn resolve_item_icon(icon: &str, fallback: Option<String>) -> Option<String> {
    if icon.is_empty() {
        return fallback;
    }
    if icon.starts_with("papirus:") {
        return find_icon_path(icon);
    }
    Some(icon.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn empty_icon_falls_back_to_plugin_icon() {
        assert_eq!(
            resolve_item_icon("", Some("/x.png".into())),
            Some("/x.png".into())
        );
        assert_eq!(resolve_item_icon("", None), None);
    }

    #[test]
    fn absolute_path_passes_through() {
        assert_eq!(
            resolve_item_icon("/home/u/icon.svg", None),
            Some("/home/u/icon.svg".into())
        );
    }

    #[test]
    fn papirus_spec_is_resolved_to_absolute_path() {
        if !Path::new("/usr/share/icons/Papirus").exists() {
            return; // theme not installed on this machine
        }
        let resolved = resolve_item_icon("papirus:folder-open", None).unwrap();
        assert!(resolved.contains("/Papirus/"));
        assert!(resolved.ends_with(".svg"));
    }
}
