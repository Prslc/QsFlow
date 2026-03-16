use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct ResultItem {
    pub title: String,
    pub summary: Option<String>,
    pub on_click: Option<String>,
    pub icon: Option<String>,
}


#[derive(Serialize, Debug, Clone)]
pub struct ThemeConfig {
    pub primary: String,      // accent_bg_color
    pub on_primary: String,   // accent_fg_color
    pub bg: String,           // window_bg_color
    pub fg: String,           // window_fg_color
    pub container: String,    // popover_bg_color
}