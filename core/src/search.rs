use anyhow::{Context, Result};
use crate::utils::{ResultItem, find_icon_path};

pub async fn search_suggestions(query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    // request
    let url = format!("https://duckduckgo.com/ac/?q={}", query);
    
    let response = reqwest::get(&url)
        .await
        .context("Failed to fetch suggestions")?;
    
    let json: Vec<serde_json::Value> = response
        .json()
        .await
        .context("Failed to parse suggestions")?;

    let mut results = Vec::new();

    results.push(ResultItem {
        title: format!("Search: {}", query),
        summary: Some("Search on DuckDuckGo".to_string()),
        on_click: Some(format!("https://duckduckgo.com/?q={}", query)),
        icon: find_icon_path("browser").or_else(|| Some("".to_string())),
    });

    // parse
    for item in json {
        if let Some(phrase) = item["phrase"].as_str() {
            results.push(ResultItem {
                title: phrase.to_string(),
                summary: Some("".to_string()),
                on_click: Some(format!("https://duckduckgo.com/?q={}", phrase)),
                icon: Some("".to_string()),
            });
        }
    }

    Ok(results)
}