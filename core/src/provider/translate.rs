use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use strum::{Display, EnumString};

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

const YOUDAO_URL: &str = "https://openapi.youdao.com/api";

pub struct Translate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString)]
pub enum Lang {
    #[strum(serialize = "auto")]
    Auto,
    #[strum(serialize = "en")]
    En,
    #[strum(serialize = "zh-CHS")]
    ZhChs,
    #[strum(serialize = "zh-CHT")]
    ZhCht,
    #[strum(serialize = "ja")]
    Ja,
    #[strum(serialize = "ko")]
    Ko,
    #[strum(serialize = "fr")]
    Fr,
    #[strum(serialize = "de")]
    De,
    #[strum(serialize = "es")]
    Es,
    #[strum(serialize = "ru")]
    Ru,
    #[strum(serialize = "pt")]
    Pt,
    #[strum(serialize = "it")]
    It,
    #[strum(serialize = "vi")]
    Vi,
    #[strum(serialize = "th")]
    Th,
    #[strum(serialize = "id")]
    Id,
    #[strum(serialize = "ar")]
    Ar,
}

struct TranslateConfig {
    app_token: String,
    app_secret: String,
    lang_from: String,
    lang_to: String,
}

impl Plugin for Translate {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "translate",
            name: "Youdao Translation",
            icon: "translator",
            ready: "Translate text via Youdao",
            keyword: "tr",
        }
    }

    fn search(
        &self,
        query: &str,
        _full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let query = query.to_string();
        Box::pin(async move { do_search(&query).await })
    }
}

async fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let icon = find_icon_path("translator")
        .or_else(|| find_icon_path("translate"))
        .or_else(|| Some(String::new()));

    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    let Some(cfg) = load_config() else {
        return Ok(vec![ResultItem {
            title: "API credentials missing".to_string(),
            summary: Some("Fill in app_token and app_secret in ~/.config/qsflow/translate.toml".to_string()),
            on_click: None,
            icon,
        }]);
    };

    match query_translate(query, &cfg).await {
        Ok(text) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
            Ok(vec![ResultItem {
                title: text,
                summary: Some("Press Enter to copy".to_string()),
                on_click: Some(format!("run:printf %s {b64} | base64 -d | wl-copy")),
                icon,
            }])
        }
        Err(err) => Ok(vec![ResultItem {
            title: "Translation failed".to_string(),
            summary: Some(format!("{err:#}")),
            on_click: None,
            icon,
        }]),
    }
}

/// Youdao v3 truncation: q unchanged up to 20 chars, otherwise
/// first 10 + total length + last 10 (characters, not bytes).
fn truncate(q: &str) -> String {
    let size = q.chars().count();
    if size <= 20 {
        q.to_string()
    } else {
        let head: String = q.chars().take(10).collect();
        let tail: String = q.chars().skip(size - 10).collect();
        format!("{head}{size}{tail}")
    }
}

/// sign = sha256(appKey + truncate(q) + salt + curtime + appSecret)
fn build_sign(app_token: &str, q: &str, salt: &str, curtime: &str, app_secret: &str) -> String {
    let input = format!("{app_token}{}{salt}{curtime}{app_secret}", truncate(q));
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

const CONFIG_FILE: &str = ".config/qsflow/translate.toml";
const CONFIG_TEMPLATE: &str = "\
# Youdao translation plugin
app_token = \"\"
app_secret = \"\"

# Optional. Defaults: Auto -> English. Uncomment to change.
# Optional. Display names: Auto, English, Chinese (Simplified),
# Chinese (Traditional), Japanese, Korean, French, German, Spanish,
# Russian, Portuguese, Italian, Vietnamese, Thai, Indonesian, Arabic
# lang_from = \"Auto\"
# lang_to = \"English\"
";

#[derive(Deserialize)]
struct RawConfig {
    app_token: Option<String>,
    app_secret: Option<String>,
    lang_from: Option<String>,
    lang_to: Option<String>,
}

fn load_config() -> Option<TranslateConfig> {
    let path = crate::system::fs::get_home().ok()?.join(CONFIG_FILE);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, CONFIG_TEMPLATE).ok();
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    parse_config(&content)
}

fn parse_config(content: &str) -> Option<TranslateConfig> {
    let raw: RawConfig = toml::from_str(content).ok()?;
    let app_token = raw.app_token.unwrap_or_default();
    let app_secret = raw.app_secret.unwrap_or_default();
    if app_token.is_empty() || app_secret.is_empty() {
        return None;
    }
    let lang_from = raw
        .lang_from
        .as_deref()
        .and_then(lang_from_display_name)
        .unwrap_or(Lang::Auto)
        .to_string();
    let lang_to = raw
        .lang_to
        .as_deref()
        .and_then(lang_from_display_name)
        .unwrap_or(Lang::En)
        .to_string();
    Some(TranslateConfig {
        app_token,
        app_secret,
        lang_from,
        lang_to,
    })
}

pub fn lang_from_display_name(name: &str) -> Option<Lang> {
    match name {
        "Auto" => Some(Lang::Auto),
        "English" => Some(Lang::En),
        "Chinese (Simplified)" => Some(Lang::ZhChs),
        "Chinese (Traditional)" => Some(Lang::ZhCht),
        "Japanese" => Some(Lang::Ja),
        "Korean" => Some(Lang::Ko),
        "French" => Some(Lang::Fr),
        "German" => Some(Lang::De),
        "Spanish" => Some(Lang::Es),
        "Russian" => Some(Lang::Ru),
        "Portuguese" => Some(Lang::Pt),
        "Italian" => Some(Lang::It),
        "Vietnamese" => Some(Lang::Vi),
        "Thai" => Some(Lang::Th),
        "Indonesian" => Some(Lang::Id),
        "Arabic" => Some(Lang::Ar),
        _ => None,
    }
}

async fn query_translate(q: &str, cfg: &TranslateConfig) -> Result<String> {
    let client = reqwest::Client::new();
    let curtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let salt = uuid::Uuid::new_v4().to_string();
    let sign = build_sign(&cfg.app_token, q, &salt, &curtime, &cfg.app_secret);

    let params = [
        ("from", cfg.lang_from.as_str()),
        ("to", cfg.lang_to.as_str()),
        ("signType", "v3"),
        ("curtime", curtime.as_str()),
        ("salt", salt.as_str()),
        ("appKey", cfg.app_token.as_str()),
        ("q", q),
        ("sign", sign.as_str()),
    ];

    let response = client
        .post(YOUDAO_URL)
        .form(&params)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .context("Failed to reach Youdao API")?;

    let json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Youdao response")?;

    json.get("translation")
        .and_then(|t| t.as_array())
        .and_then(|t| t.first())
        .and_then(|t| t.as_str())
        .map(|s| s.trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .context("Translation failed or returned empty result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_text() {
        assert_eq!(truncate("hello"), "hello");
    }

    #[test]
    fn truncate_boundary_20() {
        let s = "a".repeat(20);
        assert_eq!(truncate(&s), s);
    }

    #[test]
    fn truncate_over_20() {
        assert_eq!(truncate("abcdefghijklmnopqrstu"), "abcdefghij21lmnopqrstu");
    }

    #[test]
    fn truncate_unicode() {
        let s = "字".repeat(24);
        assert_eq!(
            truncate(&s),
            format!("{}{}{}", "字".repeat(10), 24, "字".repeat(10))
        );
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate(""), "");
    }

    #[test]
    fn sign_matches_known_vector() {
        assert_eq!(
            build_sign("t", "hello", "s", "1", "k"),
            "fa415b6e8be6a0611a2e76d4522ed23d255520e8c43de45b378a3b82930267f2"
        );
    }

    #[test]
    fn lang_from_display_name_known() {
        assert_eq!(lang_from_display_name("Auto"), Some(Lang::Auto));
        assert_eq!(
            lang_from_display_name("Chinese (Simplified)"),
            Some(Lang::ZhChs)
        );
        assert_eq!(lang_from_display_name("Klingon"), None);
    }

    #[test]
    fn lang_codes() {
        assert_eq!(Lang::ZhChs.to_string(), "zh-CHS");
        assert_eq!(Lang::En.to_string(), "en");
    }

    #[test]
    fn parse_config_minimal() {
        let cfg = parse_config("app_token = \"a\"\napp_secret = \"b\"\n").unwrap();
        assert_eq!(cfg.app_token, "a");
        assert_eq!(cfg.app_secret, "b");
        assert_eq!(cfg.lang_from, "auto");
        assert_eq!(cfg.lang_to, "en");
    }

    #[test]
    fn parse_config_missing_credentials() {
        assert!(parse_config("").is_none());
        assert!(parse_config("lang_from = \"English\"\n").is_none());
        assert!(parse_config("app_token = \"a\"\napp_secret = \"\"\n").is_none());
        assert!(parse_config("not toml = [").is_none());
    }

    #[test]
    fn parse_config_languages() {
        let cfg = parse_config(
            "app_token = \"a\"\napp_secret = \"b\"\nlang_from = \"Chinese (Simplified)\"\nlang_to = \"Japanese\"\n",
        )
        .unwrap();
        assert_eq!(cfg.lang_from, "zh-CHS");
        assert_eq!(cfg.lang_to, "ja");
    }

    #[test]
    fn empty_query_no_results() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let items = rt.block_on(do_search("  ")).unwrap();
        assert!(items.is_empty());
    }
}
