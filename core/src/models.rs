use serde::{Deserialize, Serialize};

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
}

#[derive(Serialize, Debug, Clone)]
pub struct ThemeConfig {
    pub primary: String,    // accent_bg_color
    pub on_primary: String, // accent_fg_color
    pub bg: String,         // window_bg_color
    pub fg: String,         // window_fg_color
    pub container: String,  // popover_bg_color
}
