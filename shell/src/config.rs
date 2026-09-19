use std::path::PathBuf;
use std::time::{Duration, Instant};

use wayrun_core::watch_targets;

mod model;
pub use model::{AppearanceConfig, ColorOverrides, Mode};

const DEFAULT_TEMPLATE: &str = include_str!("../default-theme.toml");

/// `~/.config/wayrun/theme.toml`. A missing file (or any missing key) keeps the
/// default, so an absent config is the shipped look.
fn theme_path() -> Option<PathBuf> {
    Some(wayrun_core::config::dir()?.join("theme.toml"))
}

/// Write the shipped template on first use so the keys are discoverable. It
/// comments every key out, so an untouched file still takes the defaults and a
/// later default change is picked up.
fn write_template() {
    let Some(path) = theme_path() else {
        return;
    };
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, DEFAULT_TEMPLATE);
}

/// A config reload is cheap, so it is debounced shorter than the core's plugin
/// reload.
const CONFIG_DEBOUNCE: Duration = Duration::from_millis(300);

/// Watch `theme.toml` and hand each parsed config to the shell's event loop.
/// Debounced: editors typically fire several events per save.
pub fn watch(tx: calloop::channel::Sender<AppearanceConfig>) -> Option<notify::RecommendedWatcher> {
    let path = theme_path()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let watch_path = path.clone();
    let now = Instant::now();
    let mut last = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // React to writes, not reads: `load` reads this file, so an unfiltered
        // Access event would re-trigger the watcher forever.
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        if !ev.paths.iter().any(|p| p == &watch_path) {
            return;
        }
        if last.elapsed() < CONFIG_DEBOUNCE {
            return;
        }
        last = Instant::now();
        let _ = tx.send(AppearanceConfig::load());
    })
    .ok()?;

    watch_targets(&mut watcher, &path);
    Some(watcher)
}
