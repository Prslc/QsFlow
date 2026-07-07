use std::future::Future;
use std::pin::Pin;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;
use anyhow::Result;

pub struct GitHub;

impl Plugin for GitHub {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "github",
            name: "GitHub",
            icon: "github",
            keyword: "g",
        }
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let query = query.to_string();
        Box::pin(async move { do_search(&query) })
    }
}

fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    Ok(vec![ResultItem {
        title: format!("GitHub: {}", query),
        summary: Some("Search code, repositories, and users".to_string()),
        on_click: Some(format!("https://github.com/search?q={}", query)),
        icon: find_icon_path("github").or_else(|| Some("".to_string())),
    }])
}
