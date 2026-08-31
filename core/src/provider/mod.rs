pub mod application;
pub mod calculator;
pub mod clipboard;
pub mod external;
pub mod file;
pub mod firefox;
pub mod runner;
pub mod system;
pub mod window;
pub mod web;

use rustc_hash::FxHashMap as HashMap;

use crate::plugin::{Meta, Plugin};

pub fn plugin_map() -> HashMap<&'static str, Box<dyn Plugin>> {
    let mut m: HashMap<&'static str, Box<dyn Plugin>> = HashMap::default();
    m.insert("calculator", Box::new(calculator::Calculator));
    m.insert("app-search", Box::new(application::AppSearch));
    m.insert("firefox-bookmarks", Box::new(firefox::FirefoxBookmarks));
    m.insert("firefox-history", Box::new(firefox::FirefoxHistory));
    m.insert("web-search", Box::new(web::WebSearch));
    m.insert("github", Box::new(external::External {
        meta: Meta {
            id: "github",
            name: "GitHub",
            icon: "github",
            ready: "Search GitHub",
            keyword: "g",
        },
        command: "qsflow-extra",
    }));
    m.insert("file-search", Box::new(file::FileSearch));
    m.insert("path-search", Box::new(file::PathSearch));
    m.insert("clipboard", Box::new(clipboard::Clipboard));
    m.insert("system-commands", Box::new(system::SystemCommands));
    m.insert("runner", Box::new(runner::Runner));
    m.insert("translate", Box::new(external::External {
        meta: Meta {
            id: "translate",
            name: "Youdao Translation",
            icon: "translator",
            ready: "Translate text via Youdao",
            keyword: "tr",
        },
        command: "qsflow-extra",
    }));
    m.insert("window", Box::new(window::Window));
    m
}
