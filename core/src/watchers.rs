use notify::Watcher;
use tokio::sync::mpsc;

/// A plugin reload re-forks every external host, so it is debounced longer than
/// a config-only reload.
const RELOAD_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);

/// Watch the parent dir (atomic rename saves) AND the file (in-place writes,
/// which a directory watch never reports); recreations arrive as create events.
pub fn watch_targets(watcher: &mut notify::RecommendedWatcher, path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = watcher.watch(dir, notify::RecursiveMode::NonRecursive);
    }
    if path.exists() {
        let _ = watcher.watch(path, notify::RecursiveMode::NonRecursive);
    }
}

/// Watch the DMS palette and re-emit a theme message to the UI on change, since
/// a resident core otherwise reads the theme once at start.
pub fn watch_theme(tx: &mpsc::Sender<String>) -> Option<notify::RecommendedWatcher> {
    let path = crate::system::theme::dms_colors_path()?;
    let tx_theme = tx.clone();
    let watch_path = path.clone();

    // Seed the dedup with the theme emitted at startup, so an unchanged file
    // never re-emits.
    let mut last_sent = serde_json::to_string(&serde_json::json!({
        "type": "theme",
        "data": crate::system::theme::load_theme(),
    }))
    .ok();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // React to content changes only; an Access event from the reads
        // `load_theme` performs would re-trigger the watcher forever.
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        if ev.paths.iter().any(|p| p == &watch_path) {
            let theme = crate::system::theme::load_theme();
            if let Ok(json) = serde_json::to_string(&serde_json::json!({
                "type": "theme",
                "data": theme,
            })) {
                // Dedup: skip unchanged themes (a single write can produce
                // several events; only the last value change should emit).
                if last_sent.as_deref() != Some(json.as_str()) {
                    last_sent = Some(json.clone());
                    // try_send: a theme message is replaceable; drop it if
                    // the queue is momentarily full rather than block.
                    let _ = tx_theme.try_send(json);
                }
            }
        }
    })
    .ok()?;

    watch_targets(&mut watcher, &path);
    Some(watcher)
}

/// Watch `config.toml` and reload behaviour; the registry is rebuilt too,
/// because a provider resolves its settings when it is built.
pub fn watch_config() -> Option<notify::RecommendedWatcher> {
    // Touch the config so the template exists and is watched from the start.
    let _ = crate::config::get();
    let path = crate::config::path()?;
    let handle = tokio::runtime::Handle::current();
    let watch_path = path.clone();
    let now = std::time::Instant::now();
    let mut last_reload = now
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or(now);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // React to writes, not reads: the reload reads this file, so an
        // unfiltered Access event would re-trigger the watcher forever.
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        if !ev.paths.iter().any(|p| p == &watch_path) {
            return;
        }
        if last_reload.elapsed() < RELOAD_DEBOUNCE {
            return;
        }
        last_reload = std::time::Instant::now();
        let handle = handle.clone();
        handle.spawn(async move {
            crate::config::reload();
            crate::plugin::reload().await;
        });
    })
    .ok()?;

    watch_targets(&mut watcher, &path);
    Some(watcher)
}

/// Watch `plugins.toml` and reload the registry (resident mode would otherwise
/// keep the startup config forever). Debounced: a save fires several events.
pub fn watch_plugins() -> Option<notify::RecommendedWatcher> {
    let path = crate::system::fs::get_home()
        .ok()?
        .join(".config/wayrun/plugins.toml");
    let handle = tokio::runtime::Handle::current();
    let watch_path = path.clone();
    let now = std::time::Instant::now();
    let mut last_reload = now
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or(now);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // React to writes, not reads: `plugin::reload` reads this file, so an
        // unfiltered Access event would re-trigger the watcher forever.
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        if !ev.paths.iter().any(|p| p == &watch_path) {
            return;
        }
        if last_reload.elapsed() < RELOAD_DEBOUNCE {
            return;
        }
        last_reload = std::time::Instant::now();
        // notify's callback runs off the runtime; hop back in to await.
        let handle = handle.clone();
        handle.spawn(async move {
            crate::plugin::reload().await;
        });
    })
    .ok()?;

    watch_targets(&mut watcher, &path);
    Some(watcher)
}
