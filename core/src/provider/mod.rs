pub mod application;
pub mod calculator;
pub mod clipboard;
pub mod external;
pub mod file;
pub mod firefox;
pub mod runner;
pub mod system;
pub mod web;
pub mod window;

use rustc_hash::FxHashMap as HashMap;

use crate::models::{ActionItem, ResultItem};
use crate::plugin::Plugin;

/// The common tail every scored provider shares: strongest score first,
/// optionally one row per title, capped at `max` results.
pub fn rank_results<T: Ord>(
    mut scored: Vec<(T, ResultItem)>,
    dedup_titles: bool,
    max: usize,
) -> Vec<ResultItem> {
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    if dedup_titles {
        scored.dedup_by(|a, b| a.1.title == b.1.title);
    }
    scored.truncate(max);
    scored.into_iter().map(|(_, item)| item).collect()
}

/// A URL row's extra command: copy the link instead of opening it. Every
/// provider whose rows are URLs shares this one action.
pub fn copy_url_action(item: &ResultItem) -> Vec<ActionItem> {
    let Some(url) = item
        .on_click
        .as_deref()
        .filter(|on_click| on_click.starts_with("http"))
    else {
        return Vec::new();
    };
    vec![ActionItem {
        title: "Copy URL".to_string(),
        on_click: format!("copy:{}", serde_json::json!({ "text": url })),
        icon: Some("edit-copy".to_string()),
    }]
}

pub fn plugin_map() -> HashMap<&'static str, Box<dyn Plugin>> {
    let mut m: HashMap<&'static str, Box<dyn Plugin>> = HashMap::default();
    m.insert("calculator", Box::new(calculator::Calculator));
    m.insert("app-search", Box::new(application::AppSearch));
    m.insert("firefox-bookmarks", Box::new(firefox::FirefoxBookmarks));
    m.insert("firefox-history", Box::new(firefox::FirefoxHistory));
    m.insert("file-search", Box::new(file::FileSearch));
    m.insert("path-search", Box::new(file::PathSearch));
    m.insert("clipboard", Box::new(clipboard::Clipboard));
    m.insert("system-commands", Box::new(system::SystemCommands));
    m.insert("runner", Box::new(runner::Runner));
    m.insert("window", Box::new(window::WindowPlugin));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(on_click: Option<&str>) -> ResultItem {
        ResultItem {
            title: "x".to_string(),
            summary: None,
            on_click: on_click.map(str::to_string),
            icon: None,
            ephemeral: false,
            actions: Vec::new(),
            badge: None,
        }
    }

    #[test]
    fn only_a_url_row_offers_a_copy_link() {
        let actions = copy_url_action(&row(Some("https://example.com")));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Copy URL");
        assert_eq!(
            actions[0].on_click,
            r#"copy:{"text":"https://example.com"}"#
        );

        assert!(copy_url_action(&row(Some("run:ls"))).is_empty());
        assert!(copy_url_action(&row(None)).is_empty());
    }
}
