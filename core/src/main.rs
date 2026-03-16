use anyhow::Result;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub mod models;
mod provider;
mod system;

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin).lines();

    // pipe
    let (tx, mut rx) = mpsc::channel::<String>(32);

    tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(json) = rx.recv().await {
            let _ = stdout.write_all(json.as_bytes()).await;
            let _ = stdout.write_all(b"\n").await;
            let _ = stdout.flush().await;
        }
    });

    let mut current_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(line) = reader.next_line().await? {
        let input = line.trim().to_string();

        // abort task
        if let Some(handle) = current_task.take() {
            handle.abort();
        }

        let tx_clone = tx.clone();

        // exec new task
        current_task = Some(tokio::spawn(async move {
            if input.is_empty() {
                let _ = tx_clone.send("[]".to_string()).await;
                return;
            }

            // exec application
            if input.starts_with("run ") {
                let cmd = input.trim_start_matches("run ").to_string();
                system::executor::execute_command(&cmd);
                return;
            }

            let (plugin_key, search_text) = if let Some((key, text)) = input.split_once(' ') {
                (key.trim(), text.trim())
            } else {
                ("", input.as_str())
            };

            let results_res = match plugin_key {
                "b" => {
                    provider::firefox::firefox_search(
                        provider::firefox::Mode::Bookmarks,
                        search_text,
                    )
                    .await
                }
                "h" => {
                    provider::firefox::firefox_search(provider::firefox::Mode::History, search_text)
                        .await
                }
                "s" => provider::web::search_suggestions(search_text).await,
                "g" => provider::github::github_search(search_text),

                _ => provider::application::search_apps(&input),
            };

            if let Ok(results) = results_res {
                if let Ok(json) = serde_json::to_string(&results) {
                    let _ = tx_clone.send(json).await;
                }
            }
        }));
    }

    Ok(())
}
