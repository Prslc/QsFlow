use std::future::Future;
use std::pin::Pin;

use crate::models::ResultItem;
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
            keyword: "s",
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
}

async fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let url = format!(
        "https://suggestqueries.google.com/complete/search?client=firefox&q={}",
        query
    );
    let response = reqwest::get(&url)
        .await
        .context("Failed to fetch suggestions")?;
    let json: Vec<serde_json::Value> = response
        .json()
        .await
        .context("Failed to parse suggestions")?;

    let mut results = vec![ResultItem {
        title: format!("Search: {}", query),
        summary: Some("Search on Google".to_string()),
        on_click: Some(format!("https://www.google.com/search?q={}", query)),
        icon: find_icon_path("google").or_else(|| Some("".to_string())),
    }];

    if let Some(suggestions) = json.get(1).and_then(|s| s.as_array()) {
        for item in suggestions {
            if let Some(phrase) = item.as_str() {
                results.push(ResultItem {
                    title: phrase.to_string(),
                    summary: Some("".to_string()),
                    on_click: Some(format!("https://www.google.com/search?q={}", phrase)),
                    icon: Some("".to_string()),
                });
            }
        }
    }

    Ok(results)
}
