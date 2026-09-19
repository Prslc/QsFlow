use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use gio::prelude::FileExt;
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
    ($name:ident, $id:literal, $display:literal, $matcher:ident, $ready:literal) => {
        pub struct $name;

        impl Plugin for $name {
            fn meta(&self) -> &Meta {
                &Meta {
                    id: $id,
                    name: $display,
                    icon: "folder",
                    ready: $ready,
                }
            }

            fn search(
                &self,
                query: &str,
                _full: &str,
            ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
                let query = query.to_lowercase();
                Box::pin(async move {
                    Ok(
                        tokio::task::spawn_blocking(move || do_search(&query, $matcher))
                            .await
                            .unwrap_or_default(),
                    )
                })
            }
        }
    };
}

search_plugin!(
    FileSearch,
    "file-search",
    "Files",
    match_name,
    "Search files by name"
);
search_plugin!(
    PathSearch,
    "path-search",
    "Paths",
    match_path,
    "Search files by path"
);

fn match_name(entry_name: &str, _entry_path: &str, query: &str) -> bool {
    entry_name.to_lowercase().contains(query)
}

fn match_path(_entry_name: &str, entry_path: &str, query: &str) -> bool {
    let path_lower = entry_path.to_lowercase();
    query
        .split_whitespace()
        .all(|token| path_lower.contains(token))
}

/// Walk filter: skip hidden dirs and build caches everywhere, and skip the
/// three roots at depth 1 of the home root (they are walked on their own) so a
/// hit under them is not emitted twice.
fn keep_entry(name: &str, depth: usize) -> bool {
    if name.starts_with('.')
        || name == "node_modules"
        || name == "target"
        || name == "__pycache__"
    {
        return false;
    }
    !(depth == 1 && matches!(name, "Desktop" | "Documents" | "Downloads"))
}

fn do_search(query: &str, matcher: fn(&str, &str, &str) -> bool) -> Vec<ResultItem> {
    if query.is_empty() {
        return vec![];
    }

    let Ok(home) = get_home() else {
        return vec![];
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
            .filter_entry(|e| keep_entry(&e.file_name().to_string_lossy(), e.depth()));

        for entry in walker.filter_map(Result::ok) {
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

            // GLib builds the URI: raw paths are invalid for spaces/non-ASCII
            let file_url = gio::File::for_path(&path).uri().to_string();

            let (title, icon) = if is_dir {
                (format!("{name}/"), "folder")
            } else {
                let icon = file_icon(&name);
                (name.into_owned(), icon)
            };

            results.push(ResultItem {
                title,
                summary: Some(path),
                on_click: Some(file_url),
                icon: find_icon_path(icon).or_else(|| Some(String::new())),
                ephemeral: false,
            });

            if results.len() >= 50 {
                break;
            }
        }

        if results.len() >= 50 {
            break;
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_nothing() {
        assert!(do_search("", match_name).is_empty());
        assert!(do_search("", match_path).is_empty());
    }

    #[test]
    fn the_home_roots_are_skipped_only_under_the_home_root() {
        // depth 1 under `~`: walked as its own root, so skip it here
        assert!(!keep_entry("Desktop", 1));
        assert!(!keep_entry("Documents", 1));
        assert!(!keep_entry("Downloads", 1));
        // a same-named dir deeper, or one directly inside a root, is kept
        assert!(keep_entry("Desktop", 2));
        assert!(keep_entry("Desktop", 0));
        assert!(keep_entry("Documents", 2));
        // build caches and dotdirs are skipped anywhere
        assert!(!keep_entry(".config", 1));
        assert!(!keep_entry("node_modules", 1));
        assert!(!keep_entry("target", 3));
        assert!(!keep_entry("__pycache__", 2));
        assert!(keep_entry("notes.txt", 2));
    }
}
