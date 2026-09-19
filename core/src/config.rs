use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

mod model;
pub use model::{Config, Font, WebSearch};

const DEFAULT_TEMPLATE: &str = include_str!("../default-config.toml");

pub fn path() -> Option<PathBuf> {
    Some(
        crate::system::fs::get_home()
            .ok()?
            .join(".config/wayrun/config.toml"),
    )
}

/// Write the shipped template on first use so the settings are discoverable.
/// Absence is already the default, so a write failure changes nothing.
fn write_template() {
    let Some(path) = path() else {
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

static CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();

fn cell() -> &'static RwLock<Config> {
    CONFIG.get_or_init(|| {
        write_template();
        RwLock::new(Config::load())
    })
}

pub fn get() -> Config {
    cell().read().expect("config lock poisoned").clone()
}

pub fn reload() {
    *cell().write().expect("config lock poisoned") = Config::load();
}

pub fn web_search_engine() -> String {
    cell()
        .read()
        .expect("config lock poisoned")
        .web_search
        .engine
        .clone()
}
