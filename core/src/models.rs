use serde::{Deserialize, Serialize};

/// One secondary command of a result row, surfaced by the shell's action panel
/// (Shift+Enter) and never run by Enter. `on_click` uses the same schemes as a
/// row's, plus the panel-only `pin:`/`unpin:`/`forget:`/`reveal:`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ActionItem {
    pub title: String,
    pub on_click: String,
    /// The icon spec; the core resolves it to an absolute path before emitting,
    /// like a row's `icon`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ResultItem {
    pub title: String,
    pub summary: Option<String>,
    pub on_click: Option<String>,
    pub icon: Option<String>,
    /// The host asked for this row not to enter usage history — a one-shot
    /// search hit, for instance. Absent on the wire means "record it".
    #[serde(default)]
    pub ephemeral: bool,
    /// Secondary commands for the row's action panel. Built-ins are attached by
    /// the core before emitting; a host may supply its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionItem>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ThemeConfig {
    pub primary: String,    // accent_bg_color
    pub on_primary: String, // accent_fg_color
    pub bg: String,         // window_bg_color
    pub fg: String,         // window_fg_color
    pub container: String,  // popover_bg_color
}
