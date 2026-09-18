//! The QsFlow backend: the plugin registry and the JSON-RPC service the shell
//! talks to. Built into the `qsflow` binary and reached with `--core`.

use anyhow::Result;

mod models;
mod plugin;
mod protocol;
mod provider;
mod rpc;
mod system;
mod watchers;

/// `--list-plugins` prints the registry and exits; otherwise serve the protocol.
async fn serve_or_list() -> Result<()> {
    if std::env::args().any(|a| a == "--list-plugins") {
        plugin::print_list().await;
        return Ok(());
    }

    protocol::serve().await
}

/// Run the core on stdin/stdout. `qsflow --core` (or invoking the binary as
/// `qsflow-core`) calls this and blocks for the process's life.
pub fn run() -> Result<()> {
    // Before the runtime exists: the env write must be single-threaded.
    system::fs::ensure_flatpak_data_dirs();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve_or_list())
}
