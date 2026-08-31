use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

/// A plugin whose results come from an external JSON-RPC subprocess (e.g.
/// `qsflow-extra`). The core stays compositor- and API-agnostic: it spawns
/// `command`, sends a `search` request, and relays the returned items.
pub struct External {
    pub meta: Meta,
    pub command: &'static str,
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
        let command = self.command.to_string();
        let plugin = self.meta.id.to_string();
        let query = query.to_string();
        let icon = self.meta.icon.to_string();
        Box::pin(async move { query_external(&command, &plugin, &query, &icon).await })
    }
}

async fn query_external(
    command: &str,
    plugin: &str,
    text: &str,
    icon: &str,
) -> Result<Vec<ResultItem>> {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "search",
        "params": { "plugin": plugin, "text": text },
        "id": 1,
    });
    let Ok(mut req_str) = serde_json::to_string(&req) else {
        return Ok(Vec::new());
    };
    req_str.push('\n');

    let mut child = match Command::new(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()), // command not found -> no results
    };

    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(req_str.as_bytes()).await.is_err() {
            let _ = child.kill().await;
            return Ok(Vec::new());
        }
        drop(stdin); // close stdin so the plugin sees EOF (one-shot)
    }

    // `wait_with_output` drains stdout/stderr and reaps the child in one step;
    // reading stdout then `wait()` separately can double-poll the join handle.
    let Ok(output) = child.wait_with_output().await else {
        return Ok(Vec::new());
    };

    // The plugin answers with one JSON-RPC response line; take the result array.
    let out = std::str::from_utf8(&output.stdout).unwrap_or_default();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(items) = v.get("result").and_then(|r| r.as_array())
        {
            let mut parsed: Vec<ResultItem> = items
                .iter()
                .filter_map(|it| serde_json::from_value(it.clone()).ok())
                .collect();

            let icon_path = find_icon_path(icon);
            for item in &mut parsed {
                if item.icon.as_deref().unwrap_or("").is_empty() {
                    item.icon = icon_path.clone();
                }
            }
            return Ok(parsed);
        }
    }
    Ok(Vec::new())
}
