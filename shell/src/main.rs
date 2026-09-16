//! `qsflow-shell` — the launcher's native frontend: an `iced` + `wlr-layer-shell`
//! overlay driving `qsflow-core`; any argument is an IPC client call instead.

mod app;
mod backend;
mod geometry;
mod ipc;
mod model;
mod theme;
mod view;

use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};

fn main() -> iced_exwlshell::Result {
    // No argument runs the daemon; any argument is a client call, so a typo
    // reaches the running instance ("error") instead of starting a second one.
    match std::env::args().nth(1) {
        None => run(),
        Some(command) => std::process::exit(ipc::client(&command)),
    }
}

fn run() -> iced_exwlshell::Result {
    // Take the IPC socket before touching Wayland: a second instance must exit
    // without ever creating a surface.
    if let Err(reason) = ipc::bind() {
        eprintln!("qsflow-shell: {reason}");
        std::process::exit(1);
    }

    let (shell_broadcast, shell_events) = iced_exwlshell::shell::channel();

    // The core owns `copy:` through wl-copy; skipping the clipboard also skips
    // an always-on Wayland worker thread.
    iced_exwlshell::disable_clipboard();

    iced_exwlshell::daemon(
        move || app::boot(shell_events.clone()),
        "QsFlow",
        app::update,
        view::view,
    )
    .settings(Settings {
        // `Background`: the runtime tears the event loop down when the last
        // surface closes unless the start mode is AllScreens/Background, so a
        // resident toggle needs it. It creates no startup surface.
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..LayerShellSettings::default()
        },
        default_font: iced::Font::with_name("Source Han Sans CN"),
        shell_broadcast,
        ..Settings::default()
    })
    .subscription(app::subscription)
    // MSAA: without it the canvas magnifier's curves are stair-stepped
    .antialiasing(true)
    // the surface must stay transparent: the launcher draws its own backdrop,
    // otherwise every frame is cleared with the theme's opaque background
    .style(|_state, theme| iced_core::theme::Style {
        background_color: iced::Color::TRANSPARENT,
        ..iced_core::theme::default(theme)
    })
    .run()
}
