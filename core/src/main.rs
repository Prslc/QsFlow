use anyhow::Result;
use std::io::{self, BufRead, Write};

mod firefox;
mod utils;

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

        let mut parts = input.splitn(2, ' ');
        let plugin_key = parts.next().unwrap_or("").trim();
        let search_text = parts.next().unwrap_or("").trim();

        let results = match plugin_key {
            "b" => firefox::search_items(firefox::Mode::Bookmarks, search_text)?,
            "h" => firefox::search_items(firefox::Mode::History, search_text)?,
            _ => vec![],
        };

        println!("{}", serde_json::to_string(&results)?);
        io::stdout().flush()?;
    }

    Ok(())
}