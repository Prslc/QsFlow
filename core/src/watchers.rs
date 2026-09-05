//! inotify watchers for config files the resident core would otherwise read
//! once at startup and never see change: the GTK theme CSS and `plugins.toml`.

use notify::Watcher;
use tokio::sync::mpsc;

/// Watch the parent dir (for atomic rename saves) AND the file itself (for
/// in-place writes, which a directory watch never reports). Deleted/recreated
/// files are still caught by the directory's create events.
fn watch_targets(watcher: &mut notify::RecommendedWatcher, path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = watcher.watch(dir, notify::RecursiveMode::NonRecursive);
    }
    if path.exists() {
        let _ = watcher.watch(path, notify::RecursiveMode::NonRecursive);
    }
}

/// The dank-colors.css path the theme is read from (None if `$HOME` unknown).
fn theme_css_path() -> Option<std::path::PathBuf> {
    let home = crate::system::fs::get_home().ok()?;
    Some(home.join(".config/gtk-4.0/dank-colors.css"))
}

/// Watch `dank-colors.css` so a GTK theme change is picked up live even while
/// the core is resident (which otherwise reads the theme once at start). On a
/// change it reloads the theme and re-emits a `{"type":"theme",...}` message to
/// the UI over the same mpsc; the UI rebinds its colors and re-renders.
pub fn watch_theme(tx: &mpsc::Sender<String>) -> Option<notify::RecommendedWatcher> {
    let path = theme_css_path()?;
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
        // Only react to content changes (write/create/rename). Access
        // events — including the reads `load_theme` itself performs — would
        // otherwise re-trigger the watcher forever.
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

/// Watch `~/.config/qsflow/plugins.toml` and reload the plugin registry on
/// change (resident mode would otherwise keep the config read at startup
/// forever). Debounced: editors typically fire several events per save.
pub fn watch_plugins() -> Option<notify::RecommendedWatcher> {
    let path = crate::system::fs::get_home()
        .ok()?
        .join(".config/qsflow/plugins.toml");
    let handle = tokio::runtime::Handle::current();
    let watch_path = path.clone();
    let mut last_reload = std::time::Instant::now() - std::time::Duration::from_secs(1);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if !ev.paths.iter().any(|p| p == &watch_path) {
            return;
        }
        if last_reload.elapsed() < std::time::Duration::from_millis(400) {
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
