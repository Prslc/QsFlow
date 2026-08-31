use anyhow::Result;
use notify::Watcher;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

mod models;
mod plugin;
mod provider;
mod rpc;
mod system;

pub(crate) async fn emit(tx: &mpsc::Sender<String>, payload: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(payload) {
        let _ = tx.send(json).await;
    }
}

/// The dank-colors.css path the theme is read from (None if `$HOME` unknown).
fn theme_css_path() -> Option<std::path::PathBuf> {
    let home = crate::system::fs::get_home().ok()?;
    Some(home.join(".config/gtk-4.0/dank-colors.css"))
}

/// Watch `dank-colors.css` so a GTK theme change is picked up live even while
/// the core is resident (which otherwise reads the theme once at start). On a
/// change it reloads the theme and re-emits a `{"type":"theme",...}` message to
/// the UI over the same mpsc; the UI rebinds its colors and re-renders.
fn watch_theme(tx: &mpsc::Sender<String>) -> Option<notify::RecommendedWatcher> {
    let path = theme_css_path()?;
    let tx_theme = tx.clone();
    let watch_path = path.clone();

    // Seed the dedup with the theme emitted at startup, so an unchanged file
    // never re-emits.
    let mut last_sent = serde_json::to_string(&serde_json::json!({
        "type": "theme",
        "data": system::theme::load_theme(),
    }))
    .ok();

    let mut watcher = notify::recommended_watcher(
        move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            // Only react to content changes (write/create/rename). Access
            // events — including the reads `load_theme` itself performs — would
            // otherwise re-trigger the watcher forever.
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                return;
            }
            if ev.paths.iter().any(|p| p == &watch_path) {
                let theme = system::theme::load_theme();
                if let Ok(json) = serde_json::to_string(&serde_json::json!({
                    "type": "theme",
                    "data": theme,
                })) {
                    // Dedup: skip unchanged themes (a single write can produce
                    // several events; only the last value change should emit).
                    if last_sent.as_deref() != Some(json.as_str()) {
                        last_sent = Some(json.clone());
                        // try_send: a theme message is replaceable; drop it if
                        // the queue is momentarily full rather than block.
                        let _ = tx_theme.try_send(json);
                    }
                }
            }
        },
    )
    .ok()?;

    // Watch the parent dir: the file may be rewritten atomically (rename), so
    // the directory is the reliable inotify target; we filter on the file above.
    if let Some(dir) = path.parent() {
        let _ = watcher.watch(dir, notify::RecursiveMode::NonRecursive);
    }
    Some(watcher)
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--list-plugins") {
        println!("{:<24} {:<24} {:<12} {:<24} STATUS", "ID", "NAME", "KEYWORD", "ICON");
        println!("{:-<24} {:-<24} {:-<12} {:-<24} {:-<8}", "", "", "", "", "");
        for (id, name, icon, keyword, enabled) in plugin::list_plugins() {
            let kw = if keyword.is_empty() { "(default)" } else { &keyword };
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
    let _theme_watcher = watch_theme(&tx);

    let mut current_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(line) = reader.next_line().await? {
        let input = line.trim_start().to_string();

        // non-search commands — handle inline, no debounce
        if input.trim().is_empty() {
            let items = system::usage::get_top(20).unwrap_or_default();
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
            let _ = system::usage::forget(input.trim_start_matches("forget "));
            continue;
        }
        if input.starts_with("run ") {
            system::executor::execute_command(input.trim_start_matches("run "));
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
