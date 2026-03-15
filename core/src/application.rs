use std::fs;
use std::env;
use std::path::PathBuf;
use walkdir::WalkDir;
use anyhow::Result;
use crate::utils::{ResultItem, find_icon_path};

pub fn search_apps(query: &str) -> Result<Vec<ResultItem>> {
    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    let mut app_dirs: Vec<PathBuf> = env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string())
        .split(':')
        .map(|s| PathBuf::from(s).join("applications"))
        .collect();
    
    if let Ok(home) = env::var("HOME") {
        app_dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }

    for dir in app_dirs {
        if !dir.exists() { continue; }

        for entry in WalkDir::new(dir).max_depth(2).into_iter().filter_map(|e| e.ok()) {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("desktop") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    let mut title = None;
                    let mut comment = None;
                    let mut exec = None;
                    let mut icon_name = None;
                    let mut no_display = false;

                    for line in content.lines() {
                        let line = line.trim();
                        if line.starts_with("NoDisplay=true") { no_display = true; break; }
                        
                        if line.starts_with("Name=") && title.is_none() { 
                            title = Some(line[5..].trim().trim_matches('"').to_string()); 
                        }
                        else if line.starts_with("Comment=") && comment.is_none() { 
                            comment = Some(line[8..].trim().trim_matches('"').to_string()); 
                        }
                        else if line.starts_with("Exec=") && exec.is_none() { 
                            // clear exec filed
                            let raw_exec = line[5..].trim().trim_matches('"');
                            let clean_exec = raw_exec.split_whitespace()
                                .filter(|s| !s.starts_with('%'))
                                .collect::<Vec<_>>()
                                .join(" ");
                            exec = Some(clean_exec); 
                        }
                        else if line.starts_with("Icon=") && icon_name.is_none() { 
                            icon_name = Some(line[5..].trim().trim_matches('"').to_string()); 
                        }
                    }

                    if no_display { continue; }

                    if let Some(t) = title {
                        // fuzzy matching
                        let is_match = query.is_empty() || 
                                     t.to_lowercase().contains(&query_lower) || 
                                     comment.as_ref().map(|c| c.to_lowercase().contains(&query_lower)).unwrap_or(false);

                        if is_match {
                            // find icon path
                            let icon_path = icon_name.and_then(|n| find_icon_path(&n));
                            
                            results.push(ResultItem {
                                title: t,
                                summary: comment,
                                on_click: exec,
                                icon: icon_path,
                            });
                        }
                    }
                }
            }
        }
    }

    results.sort_by(|a, b| a.title.cmp(&b.title));
    results.dedup_by(|a, b| a.title == b.title);
    results.truncate(50);

    Ok(results)
}