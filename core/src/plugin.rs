use std::future::Future;
use rustc_hash::FxHashMap as HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use crate::models::ResultItem;
use crate::system::icon::find_icon_path;

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
    /// External JSON-RPC host binary (resolved on PATH). When set, the plugin
    /// is NOT compiled into the core: it is spawned on demand, `search`
    /// requests are relayed verbatim, and its identity (name/icon/ready) is
    /// discovered from the host's `list_plugins` response.
    #[serde(default)]
    command: Option<String>,
}

fn default_enable() -> bool { true }

pub struct Meta {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub ready: &'static str,
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

static CONFIG: tokio::sync::RwLock<Config> =
    tokio::sync::RwLock::const_new(Config { plugins: Vec::new() });
static REGISTRY: tokio::sync::RwLock<Vec<Entry>> = tokio::sync::RwLock::const_new(Vec::new());
static REGISTRY_READY: AtomicBool = AtomicBool::new(false);
/// Serializes the one-time build and config reloads (both take INIT first).
static INIT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Lazy first build: load config + registry on first use, once.
async fn ensure_loaded() {
    if !REGISTRY_READY.load(Ordering::Acquire) {
        let _guard = INIT.lock().await;
        if !REGISTRY_READY.load(Ordering::Acquire) {
            do_reload().await;
        }
    }
}

/// Re-read `plugins.toml` and rebuild the registry. Called by the file watcher
/// so resident mode picks up config edits without a core restart; the change
/// is visible on the next search / `?` / `list_plugins`.
pub async fn reload() {
    let _guard = INIT.lock().await;
    do_reload().await;
}

async fn do_reload() {
    let new_config = load_or_default();
    let entries = build_entries(&new_config).await;
    *CONFIG.write().await = new_config;
    *REGISTRY.write().await = entries;
    REGISTRY_READY.store(true, Ordering::Release);
}

/// Build registry entries from a config. Pure w.r.t. shared state: external
/// hosts are asked once per unique command, then reused across entries that
/// share it.
async fn build_entries(config: &Config) -> Vec<Entry> {
    let mut map: PluginMap = crate::provider::plugin_map();
    let mut entries = Vec::new();
    let mut discovered: HashMap<String, Vec<crate::provider::external::HostMeta>> =
        HashMap::default();

    for p in &config.plugins {
        if !p.enable { continue; }
        if let Some(plugin) = map.remove(p.id.as_str()) {
            entries.push(Entry {
                plugin,
                keyword: p.keyword.clone(),
            });
            continue;
        }
        let Some(command) = &p.command else {
            continue; // unknown id without an external host -> skipped
        };
        if !discovered.contains_key(command) {
            discovered.insert(command.clone(), crate::provider::external::discover(command).await);
        }
        let meta = discovered
            .get(command)
            .and_then(|hosts| hosts.iter().find(|m| m.id == p.id))
            .cloned();
        entries.push(Entry {
            plugin: Box::new(crate::provider::external::External::new(
                &p.id,
                command.clone(),
                meta,
            )),
            keyword: p.keyword.clone(),
        });
    }

    entries
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

        // overlay user overrides, appending ids the defaults don't know
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(user) = toml::from_str::<Config>(&content)
        {
            config = merge_config(config, user);
        }
    }

    config
}

/// Overlay a user config onto the shipped default. Known ids are updated
/// (keyword/enable, plus `command` when the user sets one); unknown ids are
/// appended so external plugins can be declared purely from the user config
/// without touching the core. Unknown ids without a host are still skipped at
/// registry build.
fn merge_config(mut base: Config, user: Config) -> Config {
    for up in user.plugins {
        match base.plugins.iter_mut().find(|p| p.id == up.id) {
            Some(dp) => {
                dp.keyword = up.keyword;
                dp.enable = up.enable;
                if up.command.is_some() {
                    dp.command = up.command;
                }
            }
            None => base.plugins.push(up),
        }
    }
    base
}

pub async fn list_plugins() -> Vec<(String, String, String, String, bool)> {
    let map = crate::provider::plugin_map();
    ensure_loaded().await;
    let config = CONFIG.read().await;
    let reg = REGISTRY.read().await;
    config
        .plugins
        .iter()
        .filter(|p| {
            // Unknown ids without a host are ignored (per the config contract):
            // they are neither built-ins nor declared external plugins.
            p.command.is_some() || map.contains_key(p.id.as_str()) ||
                reg.iter().any(|e| e.plugin.meta().id == p.id.as_str())
        })
        .map(|p| {
            let keyword = p.keyword.clone();
            if let Some(entry) = reg.iter().find(|e| e.plugin.meta().id == p.id.as_str()) {
                let m = entry.plugin.meta();
                (
                    p.id.clone(),
                    m.name.to_string(),
                    m.icon.to_string(),
                    keyword,
                    p.enable,
                )
            } else if let Some(meta) = map.get(p.id.as_str()).map(|plugin| plugin.meta()) {
                // disabled built-in: still listed from the compiled map
                (
                    p.id.clone(),
                    meta.name.to_string(),
                    meta.icon.to_string(),
                    keyword,
                    p.enable,
                )
            } else {
                // disabled (or undiscoverable) external plugin: no host contact
                (p.id.clone(), p.id.clone(), String::new(), keyword, p.enable)
            }
        })
        .collect()
}

pub async fn dispatch(input: &str) -> Vec<ResultItem> {
    ensure_loaded().await;
    let reg = REGISTRY.read().await;

    if input.trim() == "?" {
        return reg
            .iter()
            .map(|entry| {
                let meta = entry.plugin.meta();
                let usage = if entry.keyword.is_empty() {
                    "* (default)".to_string()
                } else {
                    format!("{} <query>", entry.keyword)
                };
                ResultItem {
                    title: meta.name.to_string(),
                    summary: Some(format!("{usage} - {}", meta.ready)),
                    on_click: None,
                    icon: find_icon_path(meta.icon).or_else(|| Some(String::new())),
                }
            })
            .collect();
    }

    let (keyword, query) = input
        .split_once(' ')
        .map(|(k, q)| (k.trim(), q.trim()))
        .unwrap_or(("", input));

    if !keyword.is_empty()
        && query.is_empty()
        && let Some(entry) = reg.iter().find(|entry| entry.keyword == keyword)
    {
        let meta = entry.plugin.meta();
        return vec![ResultItem {
            title: meta.name.to_string(),
            summary: Some(meta.ready.to_string()),
            on_click: None,
            icon: find_icon_path(meta.icon).or_else(|| Some(String::new())),
        }];
    }

    // explicit keyword
    for entry in reg.iter().filter(|e| e.keyword == keyword) {
        if let Ok(results) = entry.plugin.search(query, input).await
            && !results.is_empty()
        {
            return results;
        }
    }

    // default fallback chain
    if !keyword.is_empty() {
        for entry in reg.iter().filter(|e| e.keyword.is_empty()) {
            if let Ok(results) = entry.plugin.search(query, input).await
                && !results.is_empty()
            {
                return results;
            }
        }
    }

    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Config {
        toml::from_str(src).unwrap()
    }

    #[test]
    fn user_overrides_known_ids() {
        let base = parse(
            r#"
            [[plugins]]
            id = "calculator"
            keyword = ""
            "#,
        );
        let user = parse(
            r#"
            [[plugins]]
            id = "calculator"
            keyword = "calc"
            enable = false
            "#,
        );
        let merged = merge_config(base, user);
        assert_eq!(merged.plugins.len(), 1);
        assert_eq!(merged.plugins[0].keyword, "calc");
        assert!(!merged.plugins[0].enable);
        assert!(merged.plugins[0].command.is_none());
    }

    #[test]
    fn user_appends_new_external_ids() {
        let base = parse(
            r#"
            [[plugins]]
            id = "calculator"
            keyword = ""
            "#,
        );
        let user = parse(
            r#"
            [[plugins]]
            id = "translate"
            keyword = "tr"
            command = "ext-host"
            "#,
        );
        let merged = merge_config(base, user);
        assert_eq!(merged.plugins.len(), 2);
        let external = &merged.plugins[1];
        assert_eq!(external.id, "translate");
        assert_eq!(external.keyword, "tr");
        assert_eq!(external.command.as_deref(), Some("ext-host"));
    }

    #[test]
    fn user_command_overrides_default_command() {
        let base = parse(
            r#"
            [[plugins]]
            id = "github"
            keyword = "g"
            command = "old-host"
            "#,
        );
        let user = parse(
            r#"
            [[plugins]]
            id = "github"
            keyword = "g"
            command = "ext-host"
            "#,
        );
        let merged = merge_config(base, user);
        assert_eq!(merged.plugins[0].command.as_deref(), Some("ext-host"));
        // keyword untouched when the user only sets command
        assert_eq!(merged.plugins[0].keyword, "g");
    }

    #[test]
    fn user_omitting_command_keeps_default_command() {
        let base = parse(
            r#"
            [[plugins]]
            id = "github"
            keyword = "g"
            command = "ext-host"
            "#,
        );
        let user = parse(
            r#"
            [[plugins]]
            id = "github"
            keyword = "gh"
            "#,
        );
        let merged = merge_config(base, user);
        assert_eq!(merged.plugins[0].command.as_deref(), Some("ext-host"));
        assert_eq!(merged.plugins[0].keyword, "gh");
    }
}
