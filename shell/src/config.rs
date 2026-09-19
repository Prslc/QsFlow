use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use wayrun_core::{watch as watch_file, write_if_absent};

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
///
/// Only the first `load` may write it: the watcher reloads, and an editor's
/// save briefly removes the file (`rename`, then a new one), so a reload that
/// recreated it would clobber the edit.
fn ensure_template() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        if let Some(path) = theme_path() {
            let _ = write_if_absent(&path, DEFAULT_TEMPLATE);
        }
    });
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
    watch_file(&path, CONFIG_DEBOUNCE, move || {
        let _ = tx.send(AppearanceConfig::load());
    })
}
