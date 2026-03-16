use anyhow::{Context, Result};
use dirs;
use std::env;
use std::path::PathBuf;

/// Returns the user's HOME directory.
pub fn get_home() -> Result<PathBuf> {
    dirs::home_dir().context("Failed to get user HOME directory")
}

pub fn get_project_root() -> Option<PathBuf> {
    env::current_exe()
        .ok()?
        .parent()? // bin
        .parent() // project root
        .map(|p| p.to_path_buf())
}

/// get resource path
pub fn get_resource_path(sub_path: &str) -> Option<String> {
    get_project_root()
        .map(|root| root.join(sub_path))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
}