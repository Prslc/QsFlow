//! File and path search over the home directory (`f` / `d`).
use std::future::Future;
use std::path::Path;
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

fn match_name(entry_name: &str, _entry_path: &Path, query: &str) -> bool {
    query.is_empty() || contains_ignore_ascii_case(entry_name, query)
}

/// Case-insensitive substring test: an ASCII fast path that allocates nothing
/// (the caller already lowercased the query), folding for non-ASCII input.
fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle_lower.is_ascii() {
        return haystack
            .as_bytes()
            .windows(needle_lower.len())
            .any(|window| window.eq_ignore_ascii_case(needle_lower.as_bytes()));
    }
    haystack.to_lowercase().contains(needle_lower)
}

fn match_path(_entry_name: &str, entry_path: &Path, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let path_lower = entry_path.to_string_lossy().to_lowercase();
    query
        .split_whitespace()
        .all(|token| path_lower.contains(token))
}

fn do_search(query: &str, matcher: fn(&str, &Path, &str) -> bool) -> Result<Vec<ResultItem>> {
    let home = match get_home() {
        Ok(h) => h,
        Err(_) => return Ok(vec![]),
    };

    // the three folders keep priority; the home root skips them below
    let subdirs = [
        home.join("Desktop"),
        home.join("Documents"),
        home.join("Downloads"),
    ];
    let roots = [
        subdirs[0].clone(),
        subdirs[1].clone(),
        subdirs[2].clone(),
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
                // `~` re-walks the folders that are already roots of their own
                if e.depth() == 1 && subdirs.iter().any(|subdir| subdir == e.path()) {
                    return false;
                }
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

            // match before materializing: paths are built only for hits
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !matcher(&name, entry.path(), query) {
                continue;
            }

            let path = entry.path().to_string_lossy().into_owned();

            // GLib builds the URI: raw paths are invalid for spaces/non-ASCII
            let file_url = gio::File::for_path(&path).uri().to_string();

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

#[cfg(test)]
mod tests {
    use super::contains_ignore_ascii_case;

    #[test]
    fn ascii_and_unicode_substring_matching() {
        assert!(contains_ignore_ascii_case("Report.PDF", "report"));
        assert!(!contains_ignore_ascii_case("Report", "pdf"));
        assert!(!contains_ignore_ascii_case("Report", "zzz"));
        assert!(contains_ignore_ascii_case("anything", ""));
        // Unicode falls back to the folding compare
        assert!(contains_ignore_ascii_case("Ärger", "ärger"));
        assert!(contains_ignore_ascii_case("准考证.pdf", "准考证"));
    }
}
