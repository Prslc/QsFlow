use std::path::{Path, PathBuf};

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

/// Call `on_event` for every write to `path`; with no debounce, the handler must
/// keep its state on a failed read and ignore an unchanged value.
pub fn watch<F>(path: &Path, mut on_event: F) -> Option<RecommendedWatcher>
where
    F: FnMut() + Send + 'static,
{
    let watch_path: PathBuf = path.to_path_buf();
    let filter = watch_path.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event>| {
        let Ok(ev) = res else { return };
        // React to writes, not reads: the reload reads the file, so an
        // unfiltered Access event would re-trigger the watcher forever.
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        if ev.paths.iter().any(|p| p == &filter) {
            on_event();
        }
    })
    .ok()?;

    watch_targets(&mut watcher, &watch_path);
    Some(watcher)
}
