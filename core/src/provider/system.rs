use std::future::Future;
use std::pin::Pin;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;
use anyhow::Result;

pub struct SystemCommands;

impl Plugin for SystemCommands {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "system-commands",
            name: "System Commands",
            icon: "system-shutdown",
            ready: "Search system commands",
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
        (
            "Lock",
            "lock",
            "system-lock-screen",
            "loginctl lock-session",
        ),
        ("Suspend", "suspend", "system-suspend", "systemctl suspend"),
        ("Reboot", "reboot", "system-reboot", "systemctl reboot"),
        (
            "Shutdown",
            "shutdown",
            "system-shutdown",
            "systemctl poweroff",
        ),
        (
            "Logout",
            "logout",
            "system-log-out",
            "loginctl terminate-session $XDG_SESSION_ID",
        ),
    ];

    // prefix match only: the default fallback chain short-circuits on the
    // first non-empty plugin (calculator -> system-commands -> apps), so
    // loose mid-word substring hits ("bo" inside "reBOot") shadowed
    // app-search's stronger whole-name matches and hid apps like Bottles
    commands
        .iter()
        .filter(|(name, keyword, _, _)| {
            input.is_empty() || name.to_lowercase().starts_with(input) || keyword.starts_with(input)
        })
        .map(|(name, _, icon, cmd)| ResultItem {
            title: name.to_string(),
            summary: Some(cmd.to_string()),
            on_click: Some(format!("run:{}", cmd)),
            icon: find_icon_path(icon).or_else(|| Some("".to_string())),
        })
        .collect()
}
