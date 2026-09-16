//! Entry point: pad the data dirs, build the tokio runtime, serve the protocol.
use anyhow::Result;

mod models;
mod plugin;
mod protocol;
mod provider;
mod rpc;
mod system;
mod watchers;

/// `--list-plugins` prints the registry and exits; otherwise serve the protocol.
async fn run() -> Result<()> {
    if std::env::args().any(|a| a == "--list-plugins") {
        plugin::print_list().await;
        return Ok(());
    }

    protocol::serve().await
}

fn main() -> Result<()> {
    // before the runtime exists: the env write must be single-threaded
    system::fs::ensure_flatpak_data_dirs();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}
