use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Result, Watcher};

/// Watch the parent dir (atomic rename saves) AND the file (in-place writes,
/// which a directory watch never reports); recreations arrive as create events.
fn watch_targets(watcher: &mut RecommendedWatcher, path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
    }
    if path.exists() {
        let _ = watcher.watch(path, RecursiveMode::NonRecursive);
    }
}

/// A debounced watcher for one file: it reacts only to that path's writes, at
/// most once per `debounce`, so an editor's save (several events) reloads once.
/// `debounce` of zero forwards every event.
pub fn watch<F>(path: &Path, debounce: Duration, mut on_change: F) -> Option<RecommendedWatcher>
where
    F: FnMut() + Send + 'static,
{
    let watch_path: PathBuf = path.to_path_buf();
    let filter = watch_path.clone();
    let now = Instant::now();
    let mut last = now.checked_sub(debounce).unwrap_or(now);
    let mut watcher = notify::recommended_watcher(move |res: Result<Event>| {
        let Ok(ev) = res else { return };
        // React to writes, not reads: the reload reads the file, so an
        // unfiltered Access event would re-trigger the watcher forever.
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        if !ev.paths.iter().any(|p| p == &filter) {
            return;
        }
        if last.elapsed() < debounce {
            return;
        }
        last = Instant::now();
        on_change();
    })
    .ok()?;

    watch_targets(&mut watcher, &watch_path);
    Some(watcher)
}
