//! `qsflow-shell` — the `QsFlow` launcher front end.
//!
//! With no arguments it runs the shell (resident when `QSFLOW_RESIDENT=1`); any
//! other argument is an IPC verb sent to a running instance.

mod app;
mod ime;
mod ipc;
mod platform;
mod session;
mod wayland;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    if let Some(verb) = std::env::args().nth(1) {
        match verb.as_str() {
            "open" | "close" | "toggle" | "status" => {
                return ipc::client(&verb).map_err(|err| anyhow::anyhow!("{err}"));
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // Slint has no layer-shell backend, so the shell installs its own platform:
    // the software renderer behind a `wl_shm` overlay, with the event loop owned
    // by the Wayland side.
    let adapter = platform::Adapter::new();
    slint::platform::set_platform(Box::new(platform::QsPlatform::new(adapter.clone())))
        .map_err(|_| anyhow::anyhow!("a Slint platform is already installed"))?;
    let ui = LauncherWindow::new().map_err(|err| anyhow::anyhow!("{err}"))?;
    wayland::run(ui, adapter)
}
