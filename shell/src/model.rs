//! The core's wire types: one search result and the theme payload.

/// One row of a `{"type":"results","data":[…]}` payload; absent keys are normal.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct ResultItem {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub on_click: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// The `{"type":"theme","data":{…}}` payload. `bg`/`on_primary` are not modelled
/// (unused); every field is optional, so a partial payload still applies.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ThemeConfig {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub fg: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
}
