use std::future::Future;
use std::pin::Pin;
use std::process::Command;

use anyhow::Result;
use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

pub struct Clipboard;

impl Plugin for Clipboard {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "clipboard",
            name: "Clipboard History",
            icon: "clipboard",
            keyword: "c",
        }
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let query = query.to_lowercase();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || do_search(&query))
                .await
                .unwrap_or_else(|_| Ok(vec![]))
        })
    }
}

fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let output = match Command::new("cliphist").arg("list").output() {
        Ok(o) => o,
        Err(_) => return Ok(vec![]),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in text.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        let (id, preview) = match parts.as_slice() {
            [id, _, preview] => (*id, *preview),
            [id, preview] => (*id, *preview),
            _ => continue,
        };

        if !query.is_empty() && !preview.to_lowercase().contains(&query) {
            continue;
        }

        let preview = if preview.len() > 80 {
            format!("{}…", &preview[..80])
        } else {
            preview.to_string()
        };

        results.push(ResultItem {
            title: preview,
            summary: None,
            on_click: Some(format!("run:sh -c 'cliphist decode {} | wl-copy'", id)),
            icon: find_icon_path("clipboard").or_else(|| Some("".to_string())),
        });

        if results.len() >= 50 {
            break;
        }
    }

    Ok(results)
}
