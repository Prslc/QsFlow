use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use anyhow::Context;
use serde::Deserialize;

use crate::models::ResultItem;

const DEFAULT_CONFIG: &str = r#"
[[plugins]]
id = "calculator"
keyword = ""

[[plugins]]
id = "app-search"
keyword = ""

[[plugins]]
id = "firefox-bookmarks"
keyword = "b"

[[plugins]]
id = "firefox-history"
keyword = "h"

[[plugins]]
id = "web-search"
keyword = "s"

[[plugins]]
id = "github"
keyword = "g"
"#;

#[derive(Deserialize)]
struct Config {
    plugins: Vec<PluginEntry>,
}

#[derive(Deserialize)]
struct PluginEntry {
    id: String,
    keyword: String,
}

pub struct Meta {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
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

static REGISTRY: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    let mut map: PluginMap = crate::provider::plugin_map();

    let config: Config = load_config().unwrap_or_else(|_| {
        toml::from_str(DEFAULT_CONFIG).expect("invalid default config")
    });

    let mut entries = Vec::new();
    for p in &config.plugins {
        if let Some(plugin) = map.remove(p.id.as_str()) {
            entries.push(Entry {
                plugin,
                keyword: p.keyword.clone(),
            });
        }
    }
    entries
});

fn load_config() -> anyhow::Result<Config> {
    let home = crate::system::fs::get_home()?;
    let path = home.join(".config/qsflow/plugins.toml");
    if !path.exists() {
        anyhow::bail!("no user config");
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&content).context("invalid plugin config")
}

pub async fn dispatch(input: &str) -> Vec<ResultItem> {
    let (keyword, query) = input.split_once(' ')
        .map(|(k, q)| (k.trim(), q.trim()))
        .unwrap_or(("", input));

    // explicit keyword
    for entry in REGISTRY.iter().filter(|e| e.keyword == keyword) {
        if let Ok(results) = entry.plugin.search(query, input).await {
            if !results.is_empty() {
                return results;
            }
        }
    }

    // default fallback chain
    if keyword != "" {
        for entry in REGISTRY.iter().filter(|e| e.keyword == "") {
            if let Ok(results) = entry.plugin.search(query, input).await {
                if !results.is_empty() {
                    return results;
                }
            }
        }
    }

    vec![]
}
