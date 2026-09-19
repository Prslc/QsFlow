/// One secondary command of a row, shown in the action panel (Shift+Enter).
/// `icon` is already an absolute path, resolved by the core.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct ActionItem {
    pub title: String,
    pub on_click: String,
    #[serde(default)]
    pub icon: Option<String>,
}

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
    /// The core's `ephemeral` flag: a row the plugin does not want recorded in
    /// usage history. Forwarded back in the `select` record.
    #[serde(default)]
    pub ephemeral: bool,
    /// The row's action panel, resolved by the core.
    #[serde(default)]
    pub actions: Vec<ActionItem>,
    /// A small status glyph at the row's right edge (a pin for a pinned row),
    /// already an absolute path.
    #[serde(default)]
    pub badge: Option<String>,
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
