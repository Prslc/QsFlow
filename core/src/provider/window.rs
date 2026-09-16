use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Deserialize;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

/// How long a `niri msg -j windows` answer is reused: one call is ~11ms (a
/// process spawn plus the IPC round trip), and the list only changes when the
/// user acts.
const CACHE_TTL: Duration = Duration::from_millis(250);

/// The last successful `niri msg -j windows` payload, with the time it was read.
type CachedWindows = Option<(Instant, Arc<str>)>;

static CACHE: LazyLock<Mutex<CachedWindows>> = LazyLock::new(|| Mutex::new(None));

pub struct Window;

impl Plugin for Window {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "window",
            name: "Window",
            icon: "window-duplicate",
            ready: "Switch open windows",
            keyword: "w",
        }
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let input = query.to_string();
        // `niri msg` is a blocking process spawn: keep it off the async worker
        Box::pin(async move {
            tokio::task::spawn_blocking(move || do_search(&input))
                .await
                .unwrap_or_else(|_| Ok(vec![]))
        })
    }
}

#[derive(Deserialize)]
struct WindowInfo {
    id: u64,
    title: String,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    workspace_id: Option<u64>,
}

/// List niri's windows (cached briefly) and fuzzy-match title/app_id.
/// `on_click` focuses a window by id (runs detached after the launcher exits).
fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let Some(payload) = windows_json() else {
        return Ok(Vec::new());
    };
    let Ok(windows) = serde_json::from_str::<Vec<WindowInfo>>(&payload) else {
        return Ok(Vec::new());
    };

    let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
    let pattern = nucleo::Utf32String::from(query.to_lowercase());

    let mut results: Vec<(u16, ResultItem)> = Vec::new();

    for w in windows {
        // Some apps leave the title empty (or untitled); fall back to app_id.
        let label = if w.title.trim().is_empty() {
            w.app_id.as_deref().unwrap_or_default()
        } else {
            w.title.as_str()
        };

        let score = if query.is_empty() {
            1
        } else {
            let label_utf32 = nucleo::Utf32String::from(label.to_lowercase());
            let title_score = matcher
                .fuzzy_match(label_utf32.slice(..), pattern.slice(..))
                .unwrap_or(0);
            let app_score = w
                .app_id
                .as_ref()
                .and_then(|a| {
                    let a_utf32 = nucleo::Utf32String::from(a.to_lowercase());
                    matcher.fuzzy_match(a_utf32.slice(..), pattern.slice(..))
                })
                .unwrap_or(0);
            title_score.max(app_score)
        };

        if score > 0 {
            let title = if label.is_empty() {
                String::from("Untitled")
            } else {
                label.to_string()
            };
            let summary = w.app_id.as_ref().map(|a| match w.workspace_id {
                Some(ws) => format!("{} · workspace {}", a, ws),
                None => a.clone(),
            });
            results.push((
                score,
                ResultItem {
                    title,
                    summary,
                    on_click: Some(format!("run:niri msg action focus-window --id {}", w.id)),
                    icon: w
                        .app_id
                        .as_ref()
                        .and_then(|a| find_icon_path(a))
                        .or_else(|| Some(String::new())),
                },
            ));
        }
    }

    Ok(crate::provider::rank_results(results, false, 50))
}

/// The window list as niri printed it, reused for [`CACHE_TTL`].
fn windows_json() -> Option<Arc<str>> {
    let cached = {
        let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .as_ref()
            .filter(|(at, _)| at.elapsed() < CACHE_TTL)
            .map(|(_, json)| json.clone())
    };
    if let Some(json) = cached {
        return Some(json);
    }

    let output = std::process::Command::new("niri")
        .args(["msg", "-j", "windows"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let json: Arc<str> = Arc::from(String::from_utf8_lossy(&output.stdout).into_owned());
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    *cache = Some((Instant::now(), json.clone()));
    Some(json)
}

#[cfg(test)]
mod tests {
    use super::WindowInfo;

    #[test]
    fn parses_niri_window_json() {
        let json = r##"[{"id":5,"title":"kitty","app_id":"kitty","workspace_id":1,"is_focused":false,"layout":{}}]"##;
        let windows: Vec<WindowInfo> = serde_json::from_str(json).unwrap();
        assert_eq!(windows.len(), 1);
        let w = &windows[0];
        assert_eq!(w.id, 5);
        assert_eq!(w.title, "kitty");
        assert_eq!(w.app_id.as_deref(), Some("kitty"));
        assert_eq!(w.workspace_id, Some(1));
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let json = r##"[{"id":2,"title":"foo"}]"##;
        let windows: Vec<WindowInfo> = serde_json::from_str(json).unwrap();
        assert_eq!(windows[0].app_id, None);
        assert_eq!(windows[0].workspace_id, None);
    }
}
