use anyhow::Result;
use crate::utils::{ResultItem, find_icon_path};

pub fn github_search(query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let mut results = Vec::new();

    results.push(ResultItem {
        title: format!("GitHub: {}", query),
        summary: Some("Search code, repositories, and users".to_string()),
        on_click: Some(format!("https://github.com/search?q={}", query)),
        icon: find_icon_path("github")
            .or_else(|| Some("".to_string())),
    });

    Ok(results)
}