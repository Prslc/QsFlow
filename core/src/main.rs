use anyhow::Result;
use std::io::{self, BufRead, Write};

mod firefox;
mod utils;
mod application;

fn main() -> Result<()> {
    let stdin = io::stdin();

    for line in stdin.lock().lines() {
        let input = line?;
        let input = input.trim();
        if input.is_empty() {
            println!("[]");
            io::stdout().flush()?;
            continue;
        }

        // keyword search
        let (plugin_key, search_text) = if let Some((key, text)) = input.split_once(' ') {
            (key.trim(), text.trim())
        } else {
            ("", input) // globe search
        };

        let results = match plugin_key {
            "b" => firefox::search_items(firefox::Mode::Bookmarks, search_text)?,
            "h" => firefox::search_items(firefox::Mode::History, search_text)?,
            _ => application::search_apps(input)?,
        };

        println!("{}", serde_json::to_string(&results)?);
        io::stdout().flush()?;
    }

    Ok(())
}