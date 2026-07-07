pub mod application;
pub mod calculator;
pub mod clipboard;
pub mod file;
pub mod firefox;
pub mod github;
pub mod system;
pub mod web;

use rustc_hash::FxHashMap as HashMap;

use crate::plugin::Plugin;

pub fn plugin_map() -> HashMap<&'static str, Box<dyn Plugin>> {
    let mut m: HashMap<&'static str, Box<dyn Plugin>> = HashMap::default();
    m.insert("calculator", Box::new(calculator::Calculator));
    m.insert("app-search", Box::new(application::AppSearch));
    m.insert("firefox-bookmarks", Box::new(firefox::FirefoxBookmarks));
    m.insert("firefox-history", Box::new(firefox::FirefoxHistory));
    m.insert("web-search", Box::new(web::WebSearch));
    m.insert("github", Box::new(github::GitHub));
    m.insert("file-search", Box::new(file::FileSearch));
    m.insert("path-search", Box::new(file::PathSearch));
    m.insert("clipboard", Box::new(clipboard::Clipboard));
    m.insert("system-commands", Box::new(system::SystemCommands));
    m
}
