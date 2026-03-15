use anyhow::{Context, Result};
use dirs;
use glob::glob;
use serde::Serialize;
use std::path::PathBuf;

pub fn get_firefox_db_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot get HOME dir")?;
    let pattern = home.join(".config/mozilla/firefox/*.default-release");
    let pattern_str = pattern.to_str().context("Invalid path")?;

    for entry in glob(pattern_str).context("Failed to read glob pattern")? {
        if let Ok(path) = entry {
            return Ok(path.join("places.sqlite"));
        }
    }

    anyhow::bail!("No Firefox profile found");
}


#[derive(Serialize)]
pub struct ResultItem {
    pub title: String,
    pub summary: String,
    pub on_click: Option<String>,
}