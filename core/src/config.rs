use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

const DEFAULT_TEMPLATE: &str = include_str!("../default-config.toml");

/// Core behaviour loaded from `~/.config/wayrun/config.toml`. Every field
/// defaults to the compiled-in constant, so an absent file changes nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub web_search: WebSearch,
    pub hosts: Hosts,
    pub results: Results,
    pub files: Files,
    pub history: History,
    pub font: Font,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebSearch {
    /// `google` or `duckduckgo`; an unknown value falls back to google.
    pub engine: String,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hosts {
    pub timeout_ms: u64,
    pub discovery_concurrency: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Results {
    pub apps: usize,
    pub runner: usize,
    pub windows: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Files {
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct History {
    pub top: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    /// Primary shaping family. A non-CJK family avoids mapping a CJK font when
    /// the UI never draws CJK text.
    pub family: String,
}

impl Default for WebSearch {
    fn default() -> Self {
        Self {
            engine: "google".to_string(),
            timeout_ms: 5000,
        }
    }
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            discovery_concurrency: 2,
        }
    }
}

impl Default for Results {
    fn default() -> Self {
        Self {
            apps: 50,
            runner: 20,
            windows: 50,
        }
    }
}

impl Default for Files {
    fn default() -> Self {
        Self { depth: 3 }
    }
}

impl Default for History {
    fn default() -> Self {
        Self { top: 20 }
    }
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "Source Han Sans CN".to_string(),
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    web_search: WebSearchFile,
    #[serde(default)]
    hosts: HostsFile,
    #[serde(default)]
    results: ResultsFile,
    #[serde(default)]
    files: FilesFile,
    #[serde(default)]
    history: HistoryFile,
    #[serde(default)]
    font: FontFile,
}

#[derive(serde::Deserialize, Default)]
struct WebSearchFile {
    engine: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(serde::Deserialize, Default)]
struct HostsFile {
    timeout_ms: Option<u64>,
    discovery_concurrency: Option<usize>,
}

#[derive(serde::Deserialize, Default)]
struct ResultsFile {
    apps: Option<usize>,
    runner: Option<usize>,
    windows: Option<usize>,
}

#[derive(serde::Deserialize, Default)]
struct FilesFile {
    depth: Option<usize>,
}

#[derive(serde::Deserialize, Default)]
struct HistoryFile {
    top: Option<i32>,
}

#[derive(serde::Deserialize, Default)]
struct FontFile {
    family: Option<String>,
}

impl Config {
    fn load() -> Self {
        let mut config = Self::default();
        let Some(path) = path() else {
            return config;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return config;
        };
        let Ok(file) = toml::from_str::<ConfigFile>(&text) else {
            return config;
        };
        config.apply(file);
        config
    }

    fn apply(&mut self, file: ConfigFile) {
        if let Some(engine) = file.web_search.engine {
            self.web_search.engine = engine;
        }
        if let Some(ms) = file.web_search.timeout_ms {
            self.web_search.timeout_ms = ms.clamp(100, 60_000);
        }
        if let Some(ms) = file.hosts.timeout_ms {
            self.hosts.timeout_ms = ms.clamp(100, 60_000);
        }
        if let Some(n) = file.hosts.discovery_concurrency {
            self.hosts.discovery_concurrency = n.clamp(1, 8);
        }
        if let Some(n) = file.results.apps {
            self.results.apps = n.clamp(1, 200);
        }
        if let Some(n) = file.results.runner {
            self.results.runner = n.clamp(1, 200);
        }
        if let Some(n) = file.results.windows {
            self.results.windows = n.clamp(1, 200);
        }
        if let Some(n) = file.files.depth {
            self.files.depth = n.clamp(1, 6);
        }
        if let Some(n) = file.history.top {
            self.history.top = n.clamp(1, 200);
        }
        if let Some(family) = file.font.family.filter(|f| !f.trim().is_empty()) {
            self.font.family = family;
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Config {
        let mut config = Config::default();
        config.apply(toml::from_str(src).unwrap());
        config
    }

    #[test]
    fn absent_keys_keep_the_defaults() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn present_keys_override_field_by_field() {
        let config = parse(
            r#"
            [web_search]
            engine = "duckduckgo"
            timeout_ms = 2000

            [results]
            runner = 10

            [files]
            depth = 5

            [font]
            family = "Noto Sans"
            "#,
        );
        assert_eq!(config.web_search.engine, "duckduckgo");
        assert_eq!(config.web_search.timeout_ms, 2000);
        assert_eq!(config.results.runner, 10);
        assert_eq!(config.files.depth, 5);
        assert_eq!(config.font.family, "Noto Sans");
        // untouched defaults survive
        assert_eq!(config.results.apps, 50);
        assert_eq!(config.hosts.timeout_ms, 5000);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let config = parse(
            r#"
            [hosts]
            timeout_ms = 1
            discovery_concurrency = 99

            [files]
            depth = 0

            [history]
            top = 999
            "#,
        );
        assert_eq!(config.hosts.timeout_ms, 100);
        assert_eq!(config.hosts.discovery_concurrency, 8);
        assert_eq!(config.files.depth, 1);
        assert_eq!(config.history.top, 200);
    }
}
