use anyhow::Result;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

mod models;
mod plugin;
mod provider;
mod system;

async fn emit(tx: &mpsc::Sender<String>, payload: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(payload) {
        let _ = tx.send(json).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut reader = BufReader::new(io::stdin()).lines();
    let (tx, mut rx) = mpsc::channel::<String>(32);

    tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(json) = rx.recv().await {
            let _ = stdout.write_all(json.as_bytes()).await;
            let _ = stdout.write_all(b"\n").await;
            let _ = stdout.flush().await;
        }
    });

    emit(&tx, &serde_json::json!({
        "type": "theme",
        "data": system::theme::load_theme()
    })).await;

    let mut current_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(line) = reader.next_line().await? {
        let input = line.trim().to_string();

        // non-search commands — handle inline, no debounce
        if input.is_empty() {
            let items = system::usage::get_top(20).unwrap_or_default();
            emit(&tx, &serde_json::json!({ "type": "results", "data": items })).await;
            continue;
        }
        if input.starts_with("select ") {
            let _ = system::usage::record(input.trim_start_matches("select "));
            continue;
        }
        if input.starts_with("forget ") {
            let _ = system::usage::forget(input.trim_start_matches("forget "));
            continue;
        }
        if input.starts_with("run ") {
            system::executor::execute_command(input.trim_start_matches("run "));
            continue;
        }

        // search — debounce via abort
        if let Some(handle) = current_task.take() {
            handle.abort();
        }

        let tx = tx.clone();
        current_task = Some(tokio::spawn(async move {
            let results = plugin::dispatch(&input).await;
            emit(&tx, &serde_json::json!({ "type": "results", "data": results })).await;
        }));
    }

    Ok(())
}
