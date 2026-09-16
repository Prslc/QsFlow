//! `qsflow-shell` — the launcher's native frontend: an `iced` + `wlr-layer-shell`
//! overlay that drives the `qsflow-core` backend over its line protocol.
//!
//! `qsflow-shell {open|close|toggle|status}` is the IPC client; every other
//! invocation starts the daemon.

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
        // `Background` for both modes: the runtime tears the event loop down
        // when the last surface closes unless the start mode is AllScreens or
        // Background, so a resident toggle needs it to survive a dismissal. It
        // creates no startup surface — every show is a `NewLayerShell`.
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..LayerShellSettings::default()
        },
        default_font: iced::Font::with_name("Source Han Sans CN"),
        shell_broadcast,
        ..Settings::default()
    })
    .subscription(app::subscription)
    // Antialiased geometry (MSAA): without it the canvas magnifier's curves come
    // out stair-stepped — the QML canvas had `antialiasing: true`.
    .antialiasing(true)
    // The layer surface must stay transparent: the launcher draws its own
    // backdrop and the frosted card over the blurred wallpaper. Without this
    // the runtime clears every frame with the theme's opaque background color.
    .style(|_state, theme| iced_core::theme::Style {
        background_color: iced::Color::TRANSPARENT,
        ..iced_core::theme::default(theme)
    })
    .run()
}
