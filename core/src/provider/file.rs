use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use walkdir::WalkDir;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::fs::get_home;
use crate::system::icon::find_icon_path;

pub struct FileSearch;

impl Plugin for FileSearch {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "file-search",
            name: "Files",
            icon: "folder",
            keyword: "f",
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
    let home = match get_home() {
        Ok(h) => h,
        Err(_) => return Ok(vec![]),
    };

    let roots = [
        home.join("Desktop"),
        home.join("Documents"),
        home.join("Downloads"),
        home.clone(),
    ];

    let mut results = Vec::new();

    for root in &roots {
        if !root.exists() {
            continue;
        }

        let walker = WalkDir::new(root)
            .max_depth(3)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.')
                    && name != "node_modules"
                    && name != "target"
                    && name != "__pycache__"
            });

        for entry in walker.filter_map(|e| e.ok()) {
            let ft = entry.file_type();
            let is_dir = ft.is_dir();
            if !is_dir && !ft.is_file() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_lowercase();
            if !query.is_empty() && !name.contains(query) {
                continue;
            }

            let path = entry.path().to_string_lossy().into_owned();
            let file_url = format!("file://{}", path);
            let name = entry.file_name().to_string_lossy().into_owned();

            let (title, icon) = if is_dir {
                (format!("{}/", name), "folder")
            } else {
                (name, "")
            };

            results.push(ResultItem {
                title,
                summary: Some(path),
                on_click: Some(file_url),
                icon: find_icon_path(icon).or_else(|| Some("".to_string())),
            });

            if results.len() >= 50 {
                break;
            }
        }

        if results.len() >= 50 {
            break;
        }
    }

    Ok(results)
}
