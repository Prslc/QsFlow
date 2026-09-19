use std::future::Future;
use std::pin::Pin;

use crate::models::{ActionItem, ResultItem};
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;
use anyhow::{Context, Result};

pub struct WebSearch;

impl Plugin for WebSearch {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "web-search",
            name: "Web Search",
            icon: "google",
            ready: "Search Google suggestions",
        }
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let query = query.to_string();
        Box::pin(async move { do_search(&query).await })
    }

    fn actions(&self, item: &ResultItem) -> Vec<ActionItem> {
        copy_url_action(item)
    }
}

/// A URL row's extra command: copy the link instead of opening it.
fn copy_url_action(item: &ResultItem) -> Vec<ActionItem> {
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

async fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let url = format!("https://suggestqueries.google.com/complete/search?client=firefox&q={query}");
    let response = reqwest::get(&url)
        .await
        .context("fetching web suggestions")?;
    let json: Vec<serde_json::Value> = response.json().await.context("parsing web suggestions")?;

    // one resolved engine icon shared by every row (header + suggestions)
    let icon = find_icon_path("google").unwrap_or_default();

    let mut results = vec![ResultItem {
        title: format!("Search: {query}"),
        summary: Some("Search on Google".to_string()),
        on_click: Some(format!("https://www.google.com/search?q={query}")),
        icon: Some(icon.clone()),
        ephemeral: true,
        actions: Vec::new(),
        badge: None,
    }];

    if let Some(suggestions) = json.get(1).and_then(|s| s.as_array()) {
        // Same engine icon and summary as the header row, so every suggestion
        // renders uniformly.
        results.extend(
            suggestions
                .iter()
                .filter_map(|item| item.as_str())
                .map(|phrase| ResultItem {
                    title: phrase.to_string(),
                    summary: Some("Search on Google".to_string()),
                    on_click: Some(format!("https://www.google.com/search?q={phrase}")),
                    icon: Some(icon.clone()),
                    ephemeral: true,
                    actions: Vec::new(),
                    badge: None,
                }),
        );
    }

    Ok(results)
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
            ephemeral: true,
            actions: Vec::new(),
            badge: None,
        }
    }

    #[test]
    fn a_url_row_offers_a_copy_link() {
        let actions: Vec<ActionItem> = copy_url_action(&row(Some("https://example.com")));
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
