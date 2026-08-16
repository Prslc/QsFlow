use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use walkdir::WalkDir;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::fs::get_home;
use crate::system::icon::find_icon_path;

fn file_icon(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "bmp" | "ico" => "image-x-generic",
        "mp4" | "mkv" | "avi" | "webm" | "mov" | "flv" => "video-x-generic",
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "opus" => "audio-x-generic",
        "pdf" => "application-pdf",
        "zip" | "tar" | "gz" | "rar" | "7z" | "bz2" | "xz" => "package-x-generic",
        "txt" | "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "java" | "go" | "rb" | "lua"
        | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" | "json" | "xml" | "html" | "css"
        | "md" | "conf" | "ini" | "cfg" | "log" => "text-x-generic",
        _ => "text-x-generic",
    }
}

macro_rules! search_plugin {
    ($name:ident, $id:literal, $display:literal, $kw:literal, $matcher:ident, $ready:literal) => {
        pub struct $name;

        impl Plugin for $name {
            fn meta(&self) -> &Meta {
                &Meta {
                    id: $id,
                    name: $display,
                    icon: "folder",
                    ready: $ready,
                    keyword: $kw,
                }
            }

            fn search(
                &self,
                query: &str,
                _full: &str,
            ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
                let query = query.to_lowercase();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || do_search(&query, $matcher))
                        .await
                        .unwrap_or_else(|_| Ok(vec![]))
                })
            }
        }
    };
}

search_plugin!(
    FileSearch,
    "file-search",
    "Files",
    "f",
    match_name,
    "Search files by name"
);
search_plugin!(
    PathSearch,
    "path-search",
    "Paths",
    "d",
    match_path,
    "Search files by path"
);

fn match_name(entry_name: &str, _entry_path: &str, query: &str) -> bool {
    query.is_empty() || entry_name.to_lowercase().contains(query)
}

fn match_path(_entry_name: &str, entry_path: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let path_lower = entry_path.to_lowercase();
    query
        .split_whitespace()
        .all(|token| path_lower.contains(token))
}

fn do_search(query: &str, matcher: fn(&str, &str, &str) -> bool) -> Result<Vec<ResultItem>> {
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

            let path = entry.path().to_string_lossy().into_owned();
            let name = entry.file_name().to_string_lossy();

            if !matcher(&name, &path, query) {
                continue;
            }

            let file_url = format!("file://{}", path);

            let (title, icon) = if is_dir {
                (format!("{}/", name), "folder")
            } else {
                let icon = file_icon(&name);
                (name.into_owned(), icon)
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
