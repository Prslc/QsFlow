//! The Wayland side of the launcher: a `wlr-layer-shell` overlay that presents
//! Slint's software-rendered frames through `wl_shm`, plus the seat, IPC and
//! backend plumbing around it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use i_slint_core::input::KeyEvent as SlintKeyEvent;
use i_slint_core::input::{InternalKeyEvent, KeyEventType};
use i_slint_core::window::InputMethodRequest;
use slint::LogicalPosition;
use slint::platform::software_renderer::PremultipliedRgbaColor;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent as SctkKeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{self, ZwpTextInputV3},
};

use crate::app::App;
use crate::ime::{Applied, Pending};
use crate::ipc::{self, Command};
use crate::platform::Adapter;
use crate::session::Session;
use crate::{LauncherWindow, session::Event};

/// The row height and the window size in the card's own geometry — the wheel is
/// converted from pixels to rows with the same 64px step the list uses.
const ROW_H: f64 = 64.0;

/// What a Slint callback asked for. Callbacks run inside Slint's event
/// dispatch, where the shell is already borrowed, so they only queue intent.
#[derive(Debug)]
enum Intent {
    Search(String),
    Activate(usize),
    Forget,
    Dismiss,
    MoveBy(i32),
}

type Intents = Rc<RefCell<Vec<Intent>>>;

pub struct Shell {
    ui: LauncherWindow,
    adapter: Rc<Adapter>,
    intents: Intents,
    app: App,
    session: Session,

    qh: QueueHandle<Shell>,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    format: wl_shm::Format,

    conn: Connection,
    ime_manager: Option<ZwpTextInputManagerV3>,
    ime: Option<ZwpTextInputV3>,
    pending: Pending,
    /// A composition is open, so the input method owns the keyboard.
    composing: bool,

    layer: Option<LayerSurface>,
    pool: Option<SlotPool>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,

    /// Logical (surface) size and the compositor's buffer scale; the buffer is
    /// the product of the two.
    logical_w: u32,
    logical_h: u32,
    scale: f64,
    width: u32,
    height: u32,

    visible: bool,
    configured: bool,
    frame_pending: bool,
    resident: bool,
    exit: bool,
    dismiss_at: Option<Instant>,
    hover: Option<(f64, f64)>,
    wheel: f64,
}

pub fn run(ui: LauncherWindow, adapter: Rc<Adapter>) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env().context("no Wayland display")?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor is missing")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("zwlr_layer_shell_v1 is missing")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm is missing")?;
    // ABGR8888 is byte-for-byte the renderer's premultiplied RGBA8 output; the
    // mandatory ARGB8888 fallback costs one channel swap per pixel.
    let format = if shm.formats().contains(&wl_shm::Format::Abgr8888) {
        wl_shm::Format::Abgr8888
    } else {
        wl_shm::Format::Argb8888
    };

    // Text input is optional: a compositor without the global keeps a plain
    // (Latin-only) field, which is what the QML shell did on such a session.
    let ime_manager: Option<ZwpTextInputManagerV3> = globals
        .bind(&qh, 1..=1, ())
        .map_err(|err| log::warn!("no zwp_text_input_manager_v3: {err}"))
        .ok();

    let (core_tx, core_rx) = calloop::channel::channel();
    let (ipc_tx, ipc_rx) = calloop::channel::channel();
    let session = Session::spawn(core_tx).context("spawn qsflow-core")?;

    let resident = std::env::var("QSFLOW_RESIDENT").is_ok_and(|value| value == "1");
    let reduce_motion = std::env::var("QSFLOW_REDUCED_MOTION").is_ok_and(|value| value == "1");
    ui.set_reduce_motion(reduce_motion);

    let intents: Intents = Rc::new(RefCell::new(Vec::new()));
    wire_callbacks(&ui, &intents);

    let mut shell = Shell {
        ui,
        adapter,
        intents,
        app: App::new(),
        session,
        qh: qh.clone(),
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        format,
        conn: conn.clone(),
        ime_manager,
        ime: None,
        pending: Pending::default(),
        composing: false,
        layer: None,
        pool: None,
        keyboard: None,
        pointer: None,
        logical_w: 0,
        logical_h: 0,
        scale: 1.0,
        width: 0,
        height: 0,
        visible: false,
        configured: false,
        frame_pending: false,
        resident,
        exit: false,
        dismiss_at: None,
        hover: None,
        wheel: 0.0,
    };
    shell.app.install(&shell.ui);

    let mut event_loop: EventLoop<'static, Shell> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    // `WaylandSource::insert` is the helper that dispatches pending events; the
    // plain `insert_source` callback would have to call `dispatch_pending`
    // itself, and an empty callback leaves every event unread.
    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .map_err(|err| anyhow::anyhow!("wayland source: {err}"))?;
    handle
        .insert_source(core_rx, |event, _, shell: &mut Shell| {
            if let calloop::channel::Event::Msg(event) = event {
                shell.on_core_event(event);
            }
        })
        .map_err(|err| anyhow::anyhow!("core channel: {err}"))?;
    handle
        .insert_source(ipc_rx, |event, _, shell: &mut Shell| {
            if let calloop::channel::Event::Msg(command) = event {
                shell.on_command(command);
            }
        })
        .map_err(|err| anyhow::anyhow!("ipc channel: {err}"))?;

    // A live listener means another shell owns the surface: refuse to start
    // rather than steal the socket and leave that one holding a stale surface.
    if ipc::client("status").is_ok() {
        anyhow::bail!("another qsflow-shell is already listening on the socket");
    }
    ipc::serve(ipc_tx).context("bind the IPC socket")?;

    if !shell.resident {
        shell.open();
    }

    while !shell.exit {
        if let Some(at) = shell.dismiss_at
            && Instant::now() >= at
        {
            shell.dismiss_at = None;
            shell.dismiss();
        }
        slint::platform::update_timers_and_animations();
        shell.drain_intents();
        shell.sync_ime();
        shell.render(&qh);
        if shell.exit {
            break;
        }
        let timeout = shell.next_timeout();
        if let Err(err) = event_loop.dispatch(timeout, &mut shell) {
            log::error!("event loop stopped: {err}");
            break;
        }
    }
    Ok(())
}

fn wire_callbacks(ui: &LauncherWindow, intents: &Intents) {
    let queue = intents.clone();
    ui.on_search(move |text| queue.borrow_mut().push(Intent::Search(text.to_string())));
    let queue = intents.clone();
    ui.on_activate(move |index| {
        queue
            .borrow_mut()
            .push(Intent::Activate(index.max(0) as usize))
    });
    let queue = intents.clone();
    ui.on_forget(move || queue.borrow_mut().push(Intent::Forget));
    let queue = intents.clone();
    ui.on_dismiss(move || queue.borrow_mut().push(Intent::Dismiss));
    let queue = intents.clone();
    ui.on_move_by(move |delta| queue.borrow_mut().push(Intent::MoveBy(delta)));
}

impl Shell {
    fn drain_intents(&mut self) {
        let intents: Vec<Intent> = std::mem::take(&mut self.intents.borrow_mut());
        for intent in intents {
            match intent {
                Intent::Search(text) => {
                    self.ui.set_keyword(keyword(&text).into());
                    self.session.search(&text);
                }
                Intent::MoveBy(delta) => {
                    self.app.move_by(delta);
                    self.app.publish(&self.ui);
                }
                Intent::Activate(index) => self.activate(index),
                Intent::Forget => self.forget_current(),
                Intent::Dismiss => self.dismiss(),
            }
        }
    }

    fn on_core_event(&mut self, event: Event) {
        match event {
            Event::Theme(data) => self.ui.set_theme(crate::app::theme_from(&data)),
            Event::Results { items, payload } => {
                self.app.apply_results(&self.ui, items, payload);
            }
            Event::Forgotten { id, forgotten } => {
                if let Some((pending, index)) = self.app.pending_forget
                    && pending == id
                {
                    self.app.pending_forget = None;
                    if forgotten {
                        self.app.remove_row(&self.ui, index);
                    }
                }
            }
            Event::CoreExited => {
                log::error!("qsflow-core exited");
                self.app.clear(&self.ui);
                self.exit = true;
            }
        }
    }

    fn on_command(&mut self, command: Command) {
        match command {
            Command::Open => self.open(),
            Command::Close => self.dismiss(),
            Command::Toggle => {
                if self.visible {
                    self.dismiss();
                } else {
                    self.open();
                }
            }
            Command::Status(reply) => {
                let state = if self.visible { "visible" } else { "hidden" };
                let _ = reply.send(state.to_owned());
            }
        }
    }

    fn open(&mut self) {
        if self.visible {
            return;
        }
        self.visible = true;
        self.configured = false;
        self.frame_pending = false;
        self.wheel = 0.0;
        // A show lands on the usage-ranked history, not the previous query.
        self.app.selected = 0;
        self.app.first_row = 0;
        self.ui.set_query("".into());
        self.ui.set_keyword("".into());
        self.app.publish(&self.ui);
        self.ui.set_shown(true);
        self.adapter.window().request_redraw();

        let qh = self.qh.clone();
        let surface = self.compositor.create_surface(&qh);
        let layer = self.layer_shell.create_layer_surface(
            &qh,
            surface,
            Layer::Overlay,
            Some("QsFlow"),
            None,
        );
        // Full-output, and above every other layer: `-1` is the exclusive
        // zone's "ignore", which keeps niri from reserving the top bar's strip
        // and pushing this surface below it.
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(0, 0);
        layer.commit();
        self.layer = Some(layer);
        self.session.search("");
    }

    fn dismiss(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.configured = false;
        self.frame_pending = false;
        self.ui.set_shown(false);
        // Dropping the layer surface destroys it; there is no hide verb.
        self.disable_ime();
        self.layer = None;
        self.pool = None;
        if !self.resident {
            self.exit = true;
        }
    }

    fn activate(&mut self, index: usize) {
        self.app.select(index);
        self.app.publish(&self.ui);
        let Some(item) = self.app.selected_item().cloned() else {
            return;
        };
        if item.on_click.is_empty() {
            return;
        }
        let record = serde_json::json!({
            "title": item.title,
            "summary": item.summary,
            "on_click": item.on_click,
            "icon": item.icon,
        });
        self.session.send(&format!("select {record}\n"));
        self.run_command(&item.on_click);
        // The launcher stays up for a beat so the launch does not race the
        // dismiss; a keyboard dismissal is instant.
        self.dismiss_at = Some(Instant::now() + Duration::from_millis(150));
    }

    /// Exactly one verb per row, chosen by the `on_click` scheme.
    fn run_command(&mut self, target: &str) {
        let line = if let Some(rest) = target.strip_prefix("launch:") {
            format!("launch {rest}\n")
        } else if let Some(rest) = target.strip_prefix("run:") {
            format!("run {rest}\n")
        } else if let Some(rest) = target.strip_prefix("copy:") {
            format!("copy {rest}\n")
        } else if let Some(rest) = target.strip_prefix("action:") {
            format!("action {rest}\n")
        } else if target.starts_with("http")
            || target.starts_with("file:")
            || target.starts_with("mailto:")
        {
            // The core opens URIs through GLib, which honours the portal and
            // the .desktop `Terminal=` key.
            format!("open {target}\n")
        } else {
            format!("run {target}\n")
        };
        self.session.send(&line);
    }

    fn forget_current(&mut self) {
        let Some(item) = self.app.selected_item().cloned() else {
            return;
        };
        if item.on_click.is_empty() {
            return;
        }
        // The row leaves the list only when the core says it really dropped it.
        let id = self.session.forget(&item.on_click);
        self.app.pending_forget = Some((id, self.app.selected));
    }

    /// Slint asks the input method to enable, update or disable itself through
    /// `WindowAdapterInternal`; those requests are drained and turned into
    /// `zwp_text_input_v3` calls here.
    fn sync_ime(&mut self) {
        let requests = self.adapter.take_ime_requests();
        if requests.is_empty() {
            return;
        }
        let Some(ime) = self.ime.as_ref() else { return };
        for request in requests {
            match request {
                InputMethodRequest::Disable => {
                    ime.disable();
                    ime.commit();
                }
                InputMethodRequest::Enable(properties) | InputMethodRequest::Update(properties) => {
                    ime.set_content_type(
                        zwp_text_input_v3::ContentHint::None,
                        zwp_text_input_v3::ContentPurpose::Normal,
                    );
                    ime.enable();
                    ime.set_cursor_rectangle(
                        properties.cursor_rect_origin.x as i32,
                        properties.cursor_rect_origin.y as i32,
                        properties.cursor_rect_size.width as i32,
                        properties.cursor_rect_size.height as i32,
                    );
                    ime.set_surrounding_text(
                        properties.text.to_string(),
                        properties.cursor_position as i32,
                        properties
                            .anchor_position
                            .unwrap_or(properties.cursor_position) as i32,
                    );
                    ime.commit();
                }
                _ => {}
            }
        }
        let _ = self.conn.flush();
    }

    fn enable_ime(&mut self) {
        let Some(ime) = self.ime.as_ref() else { return };
        ime.set_content_type(
            zwp_text_input_v3::ContentHint::None,
            zwp_text_input_v3::ContentPurpose::Normal,
        );
        ime.enable();
        ime.commit();
        let _ = self.conn.flush();
    }

    fn disable_ime(&mut self) {
        self.composing = false;
        self.pending = Pending::default();
        let Some(ime) = self.ime.as_ref() else { return };
        ime.disable();
        ime.commit();
        let _ = self.conn.flush();
    }

    /// A `done` closed a batch: hand it to the focused field, which draws the
    /// composition and inserts the committed text itself.
    fn apply_ime(&mut self) {
        let (event_type, key_event, preedit, selection, delete) =
            match self.pending.resolve(self.composing) {
                Applied::Idle => return,
                Applied::Preedit { text, selection } => (
                    KeyEventType::UpdateComposition,
                    SlintKeyEvent::default(),
                    text,
                    selection,
                    None,
                ),
                Applied::Commit { text, delete } => {
                    let mut key_event = SlintKeyEvent::default();
                    key_event.text = text.into();
                    (
                        KeyEventType::CommitComposition,
                        key_event,
                        String::new(),
                        None,
                        delete,
                    )
                }
            };
        self.composing = !preedit.is_empty();
        let event = InternalKeyEvent {
            key_event,
            event_type,
            preedit_text: preedit.into(),
            preedit_selection: selection.map(|(begin, end)| begin..end),
            replacement_range: delete.map(|(before, after)| -(before as i32)..after as i32),
            ..Default::default()
        };
        self.adapter
            .window()
            .dispatch_event(WindowEvent::internal(event));
    }

    fn next_timeout(&self) -> Option<Duration> {
        if let Some(at) = self.dismiss_at {
            return Some(at.saturating_duration_since(Instant::now()));
        }
        if !self.visible || self.frame_pending {
            return None;
        }
        if self.adapter.window().has_active_animations() {
            return Some(Duration::from_millis(8));
        }
        slint::platform::duration_until_next_timer_update()
    }

    /// Draws a frame into the adapter's buffer and, if there was one, presents
    /// it. Frames are paced by `wl_surface.frame`, so nothing is committed while
    /// the compositor still holds the previous one.
    fn render(&mut self, qh: &QueueHandle<Self>) {
        if !self.visible || !self.configured || self.frame_pending {
            log::trace!(
                "render skipped: visible={} configured={} frame_pending={}",
                self.visible,
                self.configured,
                self.frame_pending
            );
            return;
        }
        if !self.adapter.render_if_needed() {
            log::trace!("render skipped: nothing to redraw");
            return;
        }
        let Some(layer) = self.layer.as_ref() else {
            return;
        };
        let surface = layer.wl_surface().clone();
        let (width, height) = (self.width as i32, self.height as i32);
        let stride = width * 4;
        let adapter = self.adapter.clone();
        let pixels = adapter.pixels();
        let format = self.format;
        let Some(pool) = self.pool.as_mut() else {
            return;
        };
        let (buffer, canvas) = match pool.create_buffer(width, height, stride, format) {
            Ok(pair) => pair,
            Err(err) => {
                log::error!("shared-memory buffer failed: {err}");
                return;
            }
        };
        copy_into(&pixels, canvas, format);
        drop(pixels);

        surface.damage_buffer(0, 0, width, height);
        // The callback has to be requested before the commit it belongs to:
        // one requested afterwards waits for the next commit, which on an idle
        // launcher would be the next caret blink.
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if let Err(err) = buffer.attach_to(&surface) {
            log::error!("buffer attach failed: {err}");
            return;
        }
        if let Some(layer) = self.layer.as_ref() {
            layer.commit();
        }
        self.frame_pending = true;
        log::trace!("presented {}x{} (scale {})", width, height, self.scale);
    }

    fn apply_size(&mut self) {
        let scale = self.scale;
        self.width = (self.logical_w as f64 * scale).round() as u32;
        self.height = (self.logical_h as f64 * scale).round() as u32;
        self.adapter
            .window()
            .dispatch_event(WindowEvent::ScaleFactorChanged {
                scale_factor: scale as f32,
            });
        self.adapter
            .window()
            .set_size(slint::PhysicalSize::new(self.width, self.height));
        self.adapter.window().request_redraw();
    }

    fn key(&mut self, event_type: KeyEventType, keysym: Keysym, repeat: bool) {
        // A live composition owns the keyboard: fcitx5 reassigns Backspace and
        // the letters to itself, and a copy that also reached the field would
        // edit the query underneath it. Only the modifiers keep flowing, so
        // Slint's tracked modifier state stays balanced.
        if self.composing && !is_modifier(keysym) {
            return;
        }
        // `KeyEvent` is non-exhaustive, so it is built through `Default`.
        let mut key_event = SlintKeyEvent::default();
        key_event.text = key_text(keysym);
        key_event.repeat = repeat;
        // The runtime overwrites the modifiers from the state it tracks out of
        // the encoded modifier keys, which are dispatched as ordinary
        // press/release events.
        let event = InternalKeyEvent {
            key_event,
            event_type,
            ..Default::default()
        };
        self.adapter
            .window()
            .dispatch_event(WindowEvent::internal(event));
    }

    fn press_button(&mut self, position: (f64, f64), button: u32) {
        let button = match button {
            0x110 => PointerEventButton::Left,
            0x111 => PointerEventButton::Right,
            0x112 => PointerEventButton::Middle,
            0x113 => PointerEventButton::Back,
            0x114 => PointerEventButton::Forward,
            _ => PointerEventButton::Other,
        };
        self.adapter
            .window()
            .dispatch_event(WindowEvent::PointerPressed {
                position: LogicalPosition::new(position.0 as f32, position.1 as f32),
                button,
            });
    }

    fn release_button(&mut self, position: (f64, f64), button: u32) {
        let button = match button {
            0x110 => PointerEventButton::Left,
            0x111 => PointerEventButton::Right,
            0x112 => PointerEventButton::Middle,
            0x113 => PointerEventButton::Back,
            0x114 => PointerEventButton::Forward,
            _ => PointerEventButton::Other,
        };
        self.adapter
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: LogicalPosition::new(position.0 as f32, position.1 as f32),
                button,
            });
    }

    /// The wheel moves the selection, not just the window: a whole row per
    /// notch, with the remainder carried so a trackpad scrolls smoothly. One
    /// notch arrives as both `axis` and `axis_value120`, so only the
    /// high-resolution pair is counted when it is present.
    fn scroll(&mut self, vertical: smithay_client_toolkit::seat::pointer::AxisScroll) {
        let pixels = if vertical.value120 != 0 {
            vertical.value120 as f64 * 0.5
        } else if vertical.discrete != 0 {
            vertical.discrete as f64 * 60.0
        } else {
            vertical.absolute
        };
        if pixels == 0.0 {
            return;
        }
        if self.wheel.signum() != 0.0 && self.wheel.signum() != pixels.signum() {
            self.wheel = 0.0;
        }
        self.wheel += pixels;
        let rows = (self.wheel / ROW_H).trunc();
        if rows != 0.0 {
            self.wheel -= rows * ROW_H;
            self.app.move_by(rows as i32);
            self.app.publish(&self.ui);
        }
    }
}

/// The keys that only exist as encoded modifier events, which Slint tracks
/// itself and which must therefore keep flowing during a composition.
fn is_modifier(keysym: Keysym) -> bool {
    matches!(
        slint_key(keysym),
        Some(
            Key::Shift
                | Key::ShiftR
                | Key::Control
                | Key::ControlR
                | Key::Alt
                | Key::AltGr
                | Key::Meta
                | Key::MetaR
                | Key::CapsLock
        )
    )
}

/// Slint's `Key` codes for the keys that are not plain text.
fn slint_key(keysym: Keysym) -> Option<Key> {
    let key = if keysym == Keysym::BackSpace {
        Key::Backspace
    } else if keysym == Keysym::Tab {
        Key::Tab
    } else if keysym == Keysym::Return || keysym == Keysym::KP_Enter {
        Key::Return
    } else if keysym == Keysym::Escape {
        Key::Escape
    } else if keysym == Keysym::ISO_Left_Tab {
        Key::Backtab
    } else if keysym == Keysym::Delete {
        Key::Delete
    } else if keysym == Keysym::Shift_L {
        Key::Shift
    } else if keysym == Keysym::Shift_R {
        Key::ShiftR
    } else if keysym == Keysym::Control_L {
        Key::Control
    } else if keysym == Keysym::Control_R {
        Key::ControlR
    } else if keysym == Keysym::Alt_L || keysym == Keysym::Alt_R {
        Key::Alt
    } else if keysym == Keysym::ISO_Level3_Shift || keysym == Keysym::Mode_switch {
        Key::AltGr
    } else if keysym == Keysym::Caps_Lock {
        Key::CapsLock
    } else if keysym == Keysym::Super_L {
        Key::Meta
    } else if keysym == Keysym::Super_R {
        Key::MetaR
    } else if keysym == Keysym::Up {
        Key::UpArrow
    } else if keysym == Keysym::Down {
        Key::DownArrow
    } else if keysym == Keysym::Left {
        Key::LeftArrow
    } else if keysym == Keysym::Right {
        Key::RightArrow
    } else if keysym == Keysym::Home {
        Key::Home
    } else if keysym == Keysym::End {
        Key::End
    } else if keysym == Keysym::Page_Up {
        Key::PageUp
    } else if keysym == Keysym::Page_Down {
        Key::PageDown
    } else if keysym == Keysym::Insert {
        Key::Insert
    } else {
        return None;
    };
    Some(key)
}

fn key_text(keysym: Keysym) -> slint::SharedString {
    if let Some(key) = slint_key(keysym) {
        return key.into();
    }
    // Release events carry no UTF-8 in the protocol, so the text is derived
    // from the keysym for both directions of the same key.
    xkbcommon::xkb::keysym_to_utf8(keysym).into()
}

/// The QML's keyword chip: a one-to-three letter first word followed by a space.
fn keyword(text: &str) -> &str {
    let Some(prefix) = text.split(' ').next() else {
        return "";
    };
    if prefix.len() <= 3
        && prefix.chars().all(|c| c.is_ascii_alphabetic())
        && text.len() > prefix.len()
    {
        prefix
    } else {
        ""
    }
}

fn copy_into(pixels: &[PremultipliedRgbaColor], canvas: &mut [u8], format: wl_shm::Format) {
    let bytes = bytemuck::cast_slice::<PremultipliedRgbaColor, u8>(pixels);
    match format {
        wl_shm::Format::Abgr8888 => {
            let count = bytes.len().min(canvas.len());
            canvas[..count].copy_from_slice(&bytes[..count]);
        }
        _ => {
            for (pixel, chunk) in pixels.iter().zip(canvas.chunks_exact_mut(4)) {
                chunk[0] = pixel.blue;
                chunk[1] = pixel.green;
                chunk[2] = pixel.red;
                chunk[3] = pixel.alpha;
            }
        }
    }
}

impl CompositorHandler for Shell {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if self.layer.as_ref().map(|layer| layer.wl_surface()) != Some(surface) {
            return;
        }
        let factor = new_factor.max(1);
        surface.set_buffer_scale(factor);
        let scale = factor as f64;
        if (self.scale - scale).abs() < f64::EPSILON {
            return;
        }
        self.scale = scale;
        self.apply_size();
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.frame_pending = false;
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Shell {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Shell {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.layer.as_ref().map(|own| own.wl_surface()) == Some(layer.wl_surface()) {
            self.dismiss();
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // A configure queued for a surface that is already gone must not unlock
        // a present for the next show.
        if self.layer.as_ref().map(|own| own.wl_surface()) != Some(layer.wl_surface()) {
            return;
        }
        let (width, height) = configure.new_size;
        let (logical_w, logical_h) = if width == 0 || height == 0 {
            let info = self.output_state.outputs().next().and_then(|output| {
                self.output_state
                    .info(&output)
                    .and_then(|info| info.logical_size)
            });
            match info {
                Some((w, h)) => (w.max(1) as u32, h.max(1) as u32),
                // A compositor that refuses to size the surface still gets a
                // usable card rather than a zero-sized buffer.
                None => (1280, 720),
            }
        } else {
            (width, height)
        };
        self.logical_w = logical_w;
        self.logical_h = logical_h;
        self.apply_size();
        let bytes = (self.width as usize) * (self.height as usize) * 4;
        self.pool = match SlotPool::new(bytes, &self.shm) {
            Ok(pool) => Some(pool),
            Err(err) => {
                log::error!("shared-memory pool failed: {err}");
                return;
            }
        };
        self.configured = true;
        self.frame_pending = false;
        log::debug!(
            "configure {}x{} -> {}x{}",
            logical_w,
            logical_h,
            self.width,
            self.height
        );
        self.ui.invoke_focus_input();
        self.enable_ime();
    }
}

impl SeatHandler for Shell {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(err) => log::error!("no keyboard: {err}"),
            }
            if let Some(manager) = self.ime_manager.as_ref() {
                self.ime = Some(manager.get_text_input(&seat, qh, ()));
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(err) => log::error!("no pointer: {err}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }
        if capability == Capability::Pointer
            && let Some(pointer) = self.pointer.take()
        {
            pointer.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Shell {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if self.layer.as_ref().map(|layer| layer.wl_surface()) == Some(surface) {
            self.ui.invoke_focus_input();
            self.enable_ime();
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.layer.as_ref().map(|layer| layer.wl_surface()) == Some(surface) {
            // Nothing to do: the surface is going away or focus moved on.
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: SctkKeyEvent,
    ) {
        self.key(KeyEventType::KeyPressed, event.keysym, false);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: SctkKeyEvent,
    ) {
        self.key(KeyEventType::KeyPressed, event.keysym, true);
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: SctkKeyEvent,
    ) {
        self.key(KeyEventType::KeyReleased, event.keysym, false);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _modifiers: Modifiers,
        _raw: RawModifiers,
        _layout: u32,
    ) {
        // Slint tracks its own modifier state from the encoded modifier keys.
    }
}

impl PointerHandler for Shell {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if self.layer.as_ref().map(|layer| layer.wl_surface()) != Some(&event.surface) {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.hover = Some(event.position);
                    self.adapter
                        .window()
                        .dispatch_event(WindowEvent::PointerMoved {
                            position: LogicalPosition::new(
                                event.position.0 as f32,
                                event.position.1 as f32,
                            ),
                        });
                }
                PointerEventKind::Leave { .. } => {
                    self.hover = None;
                    self.adapter
                        .window()
                        .dispatch_event(WindowEvent::PointerExited);
                }
                PointerEventKind::Press { button, .. } => {
                    self.press_button(event.position, button);
                }
                PointerEventKind::Release { button, .. } => {
                    self.release_button(event.position, button);
                }
                PointerEventKind::Axis { vertical, .. } => self.scroll(vertical),
            }
        }
    }
}

impl Dispatch<ZwpTextInputManagerV3, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTextInputV3, ()> for Shell {
    fn event(
        state: &mut Self,
        _: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_text_input_v3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                // The protocol allows a null string where it means "empty".
                state
                    .pending
                    .preedit(text.unwrap_or_default(), cursor_begin, cursor_end);
            }
            zwp_text_input_v3::Event::CommitString { text } => {
                state.pending.commit(text.unwrap_or_default());
            }
            zwp_text_input_v3::Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => {
                state.pending.delete(before_length, after_length);
            }
            // Everything is double-buffered: only `done` applies the batch.
            zwp_text_input_v3::Event::Done { .. } => state.apply_ime(),
            _ => {}
        }
    }
}

impl ShmHandler for Shell {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Shell {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Shell);

smithay_client_toolkit::delegate_dispatch2!(Shell);
