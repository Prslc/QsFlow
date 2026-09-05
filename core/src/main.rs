use anyhow::Result;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

mod models;
mod plugin;
mod provider;
mod rpc;
mod system;
mod watchers;

pub(crate) async fn emit(tx: &mpsc::Sender<String>, payload: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(payload) {
        let _ = tx.send(json).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--list-plugins") {
        println!(
            "{:<24} {:<24} {:<12} {:<24} STATUS",
            "ID", "NAME", "KEYWORD", "ICON"
        );
        println!("{:-<24} {:-<24} {:-<12} {:-<24} {:-<8}", "", "", "", "", "");
        for (id, name, icon, keyword, enabled) in plugin::list_plugins().await {
            let kw = if keyword.is_empty() {
                "(default)"
            } else {
                &keyword
            };
            let status = if enabled { "" } else { "[disabled]" };
            println!("{id:<24} {name:<24} {kw:<12} {icon:<24}{status}");
        }
        return Ok(());
    }

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

    emit(
        &tx,
        &serde_json::json!({
            "type": "theme",
            "data": system::theme::load_theme()
        }),
    )
    .await;

    // Keep the inotify watcher alive for the core's lifetime; it re-emits the
    // theme whenever dank-colors.css changes (resident mode never sees a change
    // otherwise, since it only read the file at startup).
    let _theme_watcher = watchers::watch_theme(&tx);

    // Same for plugins.toml: reload config + registry on edit so resident mode
    // reflects keyword/enable/command changes without a core restart.
    let _plugins_watcher = watchers::watch_plugins();

    // Delete copy:-keyed usage rows recorded before the exclusion rule existed
    // (idempotent; the guard in usage::record keeps new ones out).
    let _ = system::usage::purge_ephemeral();

    let mut current_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(line) = reader.next_line().await? {
        let input = line.trim_start().to_string();

        // non-search commands — handle inline, no debounce
        if input.trim().is_empty() {
            // the whole history, not a 20-row window: with a fixed cap,
            // ⌫-curating the list kept refilling from rows hidden beyond the
            // cap, so deletions never visibly converged. The full ranked set
            // shrinks monotonically as rows are forgotten.
            let items = system::usage::get_top(i32::MAX).unwrap_or_default();
            emit(
                &tx,
                &serde_json::json!({ "type": "results", "data": items }),
            )
            .await;
            continue;
        }

        // JSON-RPC 2.0 requests — independent of the text protocol
        if rpc::handle(&input, &tx).await {
            continue;
        }

        if input.starts_with("select ") {
            let _ = system::usage::record(input.trim_start_matches("select "));
            continue;
        }
        if input.starts_with("forget ") {
            let key = input.trim_start_matches("forget ");
            let _ = system::usage::forget(key);
            plugin::forget_row(key).await;
            continue;
        }
        if input.starts_with("run ") {
            system::executor::execute_command(input.trim_start_matches("run "));
            continue;
        }
        if input.starts_with("copy ") {
            system::executor::copy_json(input.trim_start_matches("copy "));
            continue;
        }
        if input.starts_with("launch ") {
            system::executor::launch_app(input.trim_start_matches("launch "));
            continue;
        }

        // search — debounce via abort
        if let Some(handle) = current_task.take() {
            handle.abort();
        }

        let tx = tx.clone();
        current_task = Some(tokio::spawn(async move {
            let results = plugin::dispatch(&input).await;
            emit(
                &tx,
                &serde_json::json!({ "type": "results", "data": results }),
            )
            .await;
        }));
    }

    Ok(())
}
