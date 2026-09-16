//! The core's wire types: one search result and the theme payload.

/// One row of a `{"type":"results","data":[…]}` payload. Absent keys are
/// normal (the core omits `summary`/`on_click`/`icon` for rows that lack them).
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

/// The `{"type":"theme","data":{…}}` payload. The core also sends `bg` and
/// `on_primary`; the shell renders neither, so they are not modelled. Every
/// field is optional, so a partial payload degrades field-by-field instead of
/// dropping the whole message.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ThemeConfig {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub fg: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
}
