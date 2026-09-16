use std::io::Write as _;
use std::time::Duration;

use anyhow::Result;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::{plugin, rpc, system, watchers};

/// One line of the text protocol; JSON-RPC requests are handled before this.
enum Request<'a> {
    /// Empty query: the usage-ranked history.
    History,
    Select(&'a str),
    Forget(&'a str),
    Run(&'a str),
    Copy(&'a str),
    Open(&'a str),
    Launch(&'a str),
    /// `<desktop-id>:<action-id>` — one `[Desktop Action …]` group.
    Action(&'a str),
    /// Anything else is a query.
    Search(&'a str),
}

impl<'a> Request<'a> {
    fn parse(input: &'a str) -> Self {
        if input.trim().is_empty() {
            return Self::History;
        }
        // verbatim argument: `run run tests` runs "run tests"
        let Some((name, argument)) = input.split_once(' ') else {
            return Self::Search(input);
        };
        match name {
            "select" => Self::Select(argument),
            "forget" => Self::Forget(argument),
            "run" => Self::Run(argument),
            "copy" => Self::Copy(argument),
            "open" => Self::Open(argument),
            "launch" => Self::Launch(argument),
            "action" => Self::Action(argument),
            _ => Self::Search(input),
        }
    }
}

/// Serialize `payload` onto the stdout stream owned by [`spawn_writer`].
pub(crate) async fn emit(tx: &mpsc::Sender<String>, payload: &serde_json::Value) {
    if let Ok(json) = serde_json::to_string(payload) {
        let _ = tx.send(json).await;
    }
}

/// Sentinel asking the writer to flush and acknowledge (never a real payload).
const DRAIN: &str = "\u{0}";

/// Own stdout for the process lifetime: one writer thread, so no two producers
/// can interleave half a line.
///
/// Deliberately a plain thread: `tokio::io::stdout()`'s `poll_flush` panics with
/// "JoinHandle polled after completion" under a burst of output (tokio 1.53.1),
/// and a dead writer silently mutes the core.
fn spawn_writer(mut rx: mpsc::Receiver<String>) -> std::sync::mpsc::Receiver<()> {
    let (ack_tx, ack_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        // not a runtime worker thread, so `blocking_recv` is allowed here
        while let Some(json) = rx.blocking_recv() {
            if json == DRAIN {
                let _ = out.flush();
                let _ = ack_tx.send(());
                continue;
            }
            if out.write_all(json.as_bytes()).is_err()
                || out.write_all(b"\n").is_err()
            {
                break;
            }
            let _ = out.flush();
        }
    });

    ack_rx
}

/// Drain before `main` returns: a one-shot client (`printf … | qsflow-core`)
/// must still receive its last response.
async fn drain_output(tx: &mpsc::Sender<String>, ack: &std::sync::mpsc::Receiver<()>) {
    if tx.send(DRAIN.to_string()).await.is_ok() {
        let _ = ack.recv_timeout(Duration::from_millis(500));
    }
}

/// Serve the line protocol until stdin closes.
pub async fn serve() -> Result<()> {
    let (tx, rx) = mpsc::channel::<String>(32);
    let ack_rx = spawn_writer(rx);

    emit(
        &tx,
        &serde_json::json!({
            "type": "theme",
            "data": system::theme::load_theme()
        }),
    )
    .await;

    // Keep the watchers alive for the core's lifetime: resident mode re-emits
    // the theme / reloads the registry on file change instead of holding the
    // startup read forever.
    let _theme_watcher = watchers::watch_theme(&tx);

    let _plugins_watcher = watchers::watch_plugins();

    // Purge copy:-keyed rows recorded before the exclusion rule (idempotent;
    // the guard in usage::record keeps new ones out).
    let _ = system::usage::purge_ephemeral();

    let mut reader = BufReader::new(io::stdin()).lines();
    let mut search: Option<JoinHandle<()>> = None;

    while let Some(line) = reader.next_line().await? {
        let input = line.trim_start();

        // JSON-RPC 2.0 requests — independent of the text protocol
        if rpc::handle(input, &tx).await {
            continue;
        }

        match Request::parse(input) {
            Request::History => emit_history(&tx).await,
            Request::Select(item) => {
                let _ = system::usage::record(item);
            }
            Request::Forget(key) => {
                let _ = system::usage::forget(key);
                plugin::forget_row(key).await;
            }
            Request::Run(cmd) => system::executor::execute_command(cmd),
            Request::Copy(payload) => system::executor::copy_json(payload),
            Request::Open(uri) => system::executor::open_uri(uri),
            // GIO's app list is ~7ms of synchronous work: off the async worker
            Request::Launch(id) => {
                let id = id.to_string();
                let _ = tokio::task::spawn_blocking(move || {
                    system::executor::launch_app(&id);
                })
                .await;
            }
            Request::Action(spec) => system::executor::launch_desktop_action(spec),
            Request::Search(query) => start_search(&tx, &mut search, query),
        }
    }

    // stdin closed: let the writer finish before the process goes away
    drain_output(&tx, &ack_rx).await;

    Ok(())
}

/// The empty query: the full ranked history, capped only to bound the payload
/// the UI holds (`⌫` curation culls from that same payload).
const HISTORY_CAP: i32 = 1000;

async fn emit_history(tx: &mpsc::Sender<String>) {
    let items = system::usage::get_top(HISTORY_CAP).unwrap_or_default();
    emit(tx, &serde_json::json!({ "type": "results", "data": items })).await;
}

/// Keystrokes coalesce: one dispatch per quiet window. Aborting the previous
/// task is not a debounce — it stops the await, not a `spawn_blocking` provider
/// or a host process.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(60);

/// Searches supersede each other: the pending one is aborted, so only the last
/// keystroke of a burst reaches `dispatch`.
fn start_search(tx: &mpsc::Sender<String>, pending: &mut Option<JoinHandle<()>>, query: &str) {
    if let Some(handle) = pending.take() {
        handle.abort();
    }

    let tx = tx.clone();
    let query = query.to_string();
    *pending = Some(tokio::spawn(async move {
        // An empty query is the local usage history: nothing to coalesce.
        if !query.trim().is_empty() {
            tokio::time::sleep(SEARCH_DEBOUNCE).await;
        }
        let results = plugin::dispatch(&query).await;
        emit(
            &tx,
            &serde_json::json!({ "type": "results", "data": results }),
        )
        .await;
    }));
}

#[cfg(test)]
mod tests {
    use super::Request;

    #[test]
    fn parse_splits_command_and_verbatim_argument() {
        assert!(matches!(Request::parse("  "), Request::History));
        assert!(matches!(Request::parse("select {}"), Request::Select("{}")));
        assert!(matches!(
            Request::parse("open file:///tmp/x"),
            Request::Open("file:///tmp/x")
        ));
        // the argument keeps its own leading words: this runs `run tests`
        assert!(matches!(
            Request::parse("run run tests"),
            Request::Run("run tests")
        ));
        // no separator, or an unknown first word, is a query — line preserved
        assert!(matches!(Request::parse("run"), Request::Search("run")));
        assert!(matches!(
            Request::parse("firefox"),
            Request::Search("firefox")
        ));
        assert!(matches!(
            Request::parse("selects x"),
            Request::Search("selects x")
        ));
    }

    #[test]
    fn action_verb_carries_desktop_and_action_ids() {
        assert!(matches!(
            Request::parse("action org.gnome.Terminal.desktop:NewWindow"),
            Request::Action("org.gnome.Terminal.desktop:NewWindow")
        ));
        // singular "actions" is still a query, not a verb
        assert!(matches!(
            Request::parse("actions x"),
            Request::Search("actions x")
        ));
        // no separator: a bare "action" is a query
        assert!(matches!(
            Request::parse("action"),
            Request::Search("action")
        ));
    }
}
