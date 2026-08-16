use std::env;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::Result;
use walkdir::WalkDir;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

pub struct AppSearch;

impl Plugin for AppSearch {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "app-search",
            name: "Applications",
            icon: "application_default",
            ready: "Search installed applications",
            keyword: "",
        }
    }

    fn search(
        &self,
        _query: &str,
        full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let input = full.to_string();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || do_search(&input))
                .await
                .unwrap_or_else(|_| Ok(vec![]))
        })
    }
}

fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let mut results = Vec::new();
    let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
    let pattern = nucleo::Utf32String::from(query.to_lowercase());

    let mut app_dirs: Vec<PathBuf> = env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string())
        .split(':')
        .map(|s| PathBuf::from(s).join("applications"))
        .collect();

    if let Ok(home) = env::var("HOME") {
        app_dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }

    for dir in app_dirs {
        if !dir.exists() {
            continue;
        }

        for entry in WalkDir::new(dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.path().extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }

            let content = match fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let mut title = None;
            let mut comment = None;
            let mut exec = None;
            let mut icon_name = None;
            let mut no_display = false;

            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("NoDisplay=true") {
                    no_display = true;
                    break;
                }

                if line.starts_with("Name=") && title.is_none() {
                    title = Some(line[5..].trim().trim_matches('"').to_string());
                } else if line.starts_with("Comment=") && comment.is_none() {
                    comment = Some(line[8..].trim().trim_matches('"').to_string());
                } else if line.starts_with("Exec=") && exec.is_none() {
                    let raw_exec = line[5..].trim().trim_matches('"');
                    let clean_exec = raw_exec
                        .split_whitespace()
                        .filter(|s| !s.starts_with('%'))
                        .collect::<Vec<_>>()
                        .join(" ");
                    exec = Some(clean_exec);
                } else if line.starts_with("Icon=") && icon_name.is_none() {
                    icon_name = Some(line[5..].trim().trim_matches('"').to_string());
                }
            }

            if no_display {
                continue;
            }

            if let Some(t) = title {
                let score = if query.is_empty() {
                    1
                } else {
                    let title_utf32 = nucleo::Utf32String::from(t.to_lowercase());
                    let name_score = matcher
                        .fuzzy_match(title_utf32.slice(..), pattern.slice(..))
                        .unwrap_or(0);

                    let comment_score = comment
                        .as_ref()
                        .and_then(|c| {
                            let c_utf32 = nucleo::Utf32String::from(c.to_lowercase());
                            matcher.fuzzy_match(c_utf32.slice(..), pattern.slice(..))
                        })
                        .unwrap_or(0);

                    name_score.max(comment_score)
                };

                if score > 0 {
                    let icon_path = icon_name.and_then(|n| find_icon_path(&n));
                    results.push((
                        score,
                        ResultItem {
                            title: t,
                            summary: comment,
                            on_click: exec.map(|e| format!("run:{}", e)),
                            icon: icon_path,
                        },
                    ));
                }
            }
        }
    }

    results.sort_by(|a, b| b.0.cmp(&a.0));
    results.dedup_by(|a, b| a.1.title == b.1.title);
    results.truncate(50);

    Ok(results.into_iter().map(|(_, item)| item).collect())
}
