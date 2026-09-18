mod app;
mod ime;
mod ipc;
mod platform;
mod session;
mod wayland;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args();
    let invoked_as = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();

    // The shell re-execs this binary with `--core`; a `qsflow-core` symlink
    // keeps the documented stdin/JSON-RPC entry point working.
    let as_core = std::path::Path::new(&invoked_as)
        .file_name()
        .is_some_and(|name| name == "qsflow-core");
    if as_core
        || rest
            .iter()
            .any(|arg| arg == "--core" || arg == "--list-plugins")
    {
        return qsflow_core::run();
    }

    if let Some(verb) = rest.first() {
        match verb.as_str() {
            "open" | "close" | "toggle" | "status" => {
                return ipc::client(verb).map_err(|err| anyhow::anyhow!("{err}"));
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
