use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

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
            ready: "Search Google suggestions",
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

/// Hard deadline on the suggestion request: a hung connection must not hold a
/// search.
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default()
});

/// `https://…/path?key=value` with the query percent-encoded (a raw `format!`
/// let spaces, `&` and `#` break the request).
fn url_with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse(base).expect("static base URL");
    url.query_pairs_mut().extend_pairs(pairs);
    url.to_string()
}

async fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let url = url_with_query(
        "https://suggestqueries.google.com/complete/search",
        &[("client", "firefox"), ("q", query)],
    );
    let response = CLIENT
        .get(&url)
        .send()
        .await
        .context("Failed to fetch suggestions")?;
    let json: Vec<serde_json::Value> = response
        .json()
        .await
        .context("Failed to parse suggestions")?;

    // one resolved engine icon shared by every row (header + suggestions)
    let icon = find_icon_path("google").unwrap_or_default();
    let search_url = url_with_query("https://www.google.com/search", &[("q", query)]);

    let mut results = vec![ResultItem {
        title: format!("Search: {}", query),
        summary: Some("Search on Google".to_string()),
        on_click: Some(search_url),
        icon: Some(icon.clone()),
    }];

    if let Some(suggestions) = json.get(1).and_then(|s| s.as_array()) {
        // same engine icon and summary as the header row — icon-less
        // or summary-less rows would otherwise render as the UI's bare
        // 48px text-only "simple" rows and look incoherent
        results.extend(suggestions.iter().filter_map(|item| item.as_str()).map(
            |phrase| ResultItem {
                title: phrase.to_string(),
                summary: Some("Search on Google".to_string()),
                on_click: Some(url_with_query(
                    "https://www.google.com/search",
                    &[("q", phrase)],
                )),
                icon: Some(icon.clone()),
            },
        ));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::url_with_query;

    #[test]
    fn queries_are_percent_encoded() {
        let url = url_with_query(
            "https://suggestqueries.google.com/complete/search",
            &[("client", "firefox"), ("q", "c++ & rust #1")],
        );
        assert!(url.contains("client=firefox"));
        assert!(!url.contains(' '));
        assert!(!url.contains('#'));
        assert!(url.contains("c%2B%2B") || url.contains("c++"));
        assert!(url.contains("%26") || url.contains("%26"));
    }

    #[test]
    fn plain_queries_keep_their_shape() {
        let url = url_with_query("https://www.google.com/search", &[("q", "firefox")]);
        assert!(url.starts_with("https://www.google.com/search?"));
        assert!(url.ends_with("q=firefox"));
    }
}
