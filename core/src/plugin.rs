use std::future::Future;
use rustc_hash::FxHashMap as HashMap;
use std::pin::Pin;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::models::ResultItem;

const DEFAULT_CONFIG: &str = include_str!("../default-plugins.toml");

#[derive(Deserialize)]
struct Config {
    plugins: Vec<PluginEntry>,
}

#[derive(Deserialize)]
struct PluginEntry {
    id: String,
    keyword: String,
    #[serde(default = "default_enable")]
    enable: bool,
}

fn default_enable() -> bool { true }

pub struct Meta {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    #[allow(dead_code)]
    pub keyword: &'static str,
}

pub trait Plugin: Send + Sync {
    fn meta(&self) -> &Meta;
    fn search(
        &self,
        query: &str,
        full: &str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<ResultItem>>> + Send + '_>>;
}

type PluginMap = HashMap<&'static str, Box<dyn Plugin>>;

struct Entry {
    plugin: Box<dyn Plugin>,
    keyword: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();
static REGISTRY: OnceLock<Vec<Entry>> = OnceLock::new();

fn config() -> &'static Config {
    CONFIG.get_or_init(load_or_default)
}

fn registry() -> &'static Vec<Entry> {
    REGISTRY.get_or_init(|| {
        let mut map: PluginMap = crate::provider::plugin_map();
        let mut entries = Vec::new();
        for p in &config().plugins {
            if !p.enable { continue; }
            if let Some(plugin) = map.remove(p.id.as_str()) {
                entries.push(Entry {
                    plugin,
                    keyword: p.keyword.clone(),
                });
            }
        }
        entries
    })
}

fn load_or_default() -> Config {
    let mut config: Config = toml::from_str(DEFAULT_CONFIG).expect("invalid default config");

    if let Ok(home) = crate::system::fs::get_home() {
        let path = home.join(".config/qsflow/plugins.toml");

        // first run: write default config
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&path, DEFAULT_CONFIG).ok();
        }

        // overlay user keyword overrides
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(user) = toml::from_str::<Config>(&content)
        {
            for up in &user.plugins {
                if let Some(dp) = config.plugins.iter_mut().find(|p| p.id == up.id) {
                    dp.keyword = up.keyword.clone();
                    dp.enable = up.enable;
                }
            }
        }
    }

    config
}

pub fn list_plugins() -> Vec<(&'static str, &'static str, &'static str, String, bool)> {
    let map = crate::provider::plugin_map();
    config()
        .plugins
        .iter()
        .filter_map(|p| {
            let meta = map.get(p.id.as_str())?.meta();
            Some((meta.id, meta.name, meta.icon, p.keyword.clone(), p.enable))
        })
        .collect()
}

pub async fn dispatch(input: &str) -> Vec<ResultItem> {
    let (keyword, query) = input
        .split_once(' ')
        .map(|(k, q)| (k.trim(), q.trim()))
        .unwrap_or(("", input));

    // explicit keyword
    for entry in registry().iter().filter(|e| e.keyword == keyword) {
        if let Ok(results) = entry.plugin.search(query, input).await
            && !results.is_empty()
        {
            return results;
        }
    }

    // default fallback chain
    if !keyword.is_empty() {
        for entry in registry().iter().filter(|e| e.keyword.is_empty()) {
            if let Ok(results) = entry.plugin.search(query, input).await
                && !results.is_empty()
            {
                return results;
            }
        }
    }

    vec![]
}
