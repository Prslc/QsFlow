use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct ResultItem {
    pub title: String,
    pub summary: Option<String>,
    pub on_click: Option<String>,
    pub icon: Option<String>,
}