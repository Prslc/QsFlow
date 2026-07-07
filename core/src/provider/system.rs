use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

pub struct SystemCommands;

impl Plugin for SystemCommands {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "system-commands",
            name: "System Commands",
            icon: "system-shutdown",
            keyword: "",
        }
    }

    fn search(
        &self,
        _query: &str,
        full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let input = full.to_lowercase();
        Box::pin(async move { Ok(do_search(&input)) })
    }
}

fn do_search(input: &str) -> Vec<ResultItem> {
    let commands = [
        ("Lock", "lock", "system-lock-screen", "loginctl lock-session"),
        ("Suspend", "suspend", "system-suspend", "systemctl suspend"),
        ("Reboot", "reboot", "system-reboot", "systemctl reboot"),
        ("Shutdown", "shutdown", "system-shutdown", "systemctl poweroff"),
        ("Logout", "logout", "system-log-out", "loginctl terminate-session $XDG_SESSION_ID"),
    ];

    let mut results = Vec::new();
    for (name, keyword, icon, cmd) in commands {
        if !input.is_empty() && !name.to_lowercase().contains(input) && !keyword.contains(input) {
            continue;
        }
        results.push(ResultItem {
            title: name.to_string(),
            summary: Some(cmd.to_string()),
            on_click: Some(format!("run:{}", cmd)),
            icon: find_icon_path(icon).or_else(|| Some("".to_string())),
        });
    }

    results
}
