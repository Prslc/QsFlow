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
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
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
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::{self, ExtBackgroundEffectSurfaceV1},
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{self, ZwpTextInputV3},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
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

    qh: QueueHandle<Self>,
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
    /// The geometry last handed to the input method. The compositor only
    /// associates it with a surface after `enter`, and `enter` can arrive after
    /// the request that carried it, so it is kept for a re-send.
    ime_state: Option<ImeState>,
    /// Re-run focus after the first frame of a show, when the field is laid out.
    refocus_after_frame: bool,

    fractional_manager: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    fscale: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    /// The output's exact ratio; with a viewport it replaces the integer buffer
    /// scale, which would cost four times the pixels for 1.25x of detail.
    ratio: Option<f64>,

    blur_manager: Option<ExtBackgroundEffectManagerV1>,
    blur: Option<ExtBackgroundEffectSurfaceV1>,
    /// The 4px-quantized card rect the compositor was last given.
    blur_rect: Option<(i32, i32, i32, i32)>,

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
    let fractional_manager: Option<WpFractionalScaleManagerV1> = globals
        .bind(&qh, 1..=1, ())
        .map_err(|err| log::warn!("no fractional scale: {err}"))
        .ok();
    let viewporter: Option<WpViewporter> = globals
        .bind(&qh, 1..=1, ())
        .map_err(|err| log::warn!("no viewporter: {err}"))
        .ok();
    // A fractional buffer is only usable when a viewport maps it back onto the
    // surface's logical size, so the exact-scale path needs both globals.
    let fractional_manager = fractional_manager.filter(|_| viewporter.is_some());
    let blur_manager: Option<ExtBackgroundEffectManagerV1> = globals
        .bind(&qh, 1..=1, ())
        .map_err(|err| log::warn!("no background effect: {err}"))
        .ok();

    let ime_manager: Option<ZwpTextInputManagerV3> = globals
        .bind(&qh, 1..=1, ())
        .map_err(|err| log::warn!("no zwp_text_input_manager_v3: {err}"))
        .ok();

    let (core_tx, core_rx) = calloop::channel::channel();
    let (ipc_tx, ipc_rx) = calloop::channel::channel();

    // A live listener means another shell owns the surface: refuse to start
    // rather than steal the socket and leave that one holding a stale surface.
    // The check runs before the core is spawned, so a second instance does not
    // briefly start one.
    if ipc::client("status").is_ok() {
        anyhow::bail!("another qsflow is already listening on the socket");
    }
    ipc::serve(ipc_tx).context("bind the IPC socket")?;

    let session = Session::spawn(core_tx).context("spawn the core")?;

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
        ime_state: None,
        refocus_after_frame: false,
        fractional_manager,
        viewporter,
        fscale: None,
        viewport: None,
        ratio: None,
        blur_manager,
        blur: None,
        blur_rect: None,
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
    WaylandSource::new(conn, event_queue)
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
            .push(Intent::Activate(index.max(0) as usize));
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
                log::error!("the core exited");
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
        // Both scaling objects must exist before the surface's first commit:
        // the exact ratio arrives as an event on the fractional-scale one.
        self.fscale = self
            .fractional_manager
            .as_ref()
            .map(|manager| manager.get_fractional_scale(&surface, &qh, ()));
        self.viewport = self
            .viewporter
            .as_ref()
            .map(|viewporter| viewporter.get_viewport(&surface, &qh, ()));
        let layer = self.layer_shell.create_layer_surface(
            &qh,
            surface.clone(),
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
        self.blur = self
            .blur_manager
            .as_ref()
            .map(|manager| manager.get_background_effect(&surface, &qh, ()));
        self.blur_rect = None;
        layer.commit();
        self.layer = Some(layer);
        self.session.search("");
        // The clipboard can change while the launcher is hidden; read it now so
        // the first Ctrl+V of this show answers without blocking.
        crate::platform::refresh_clipboard();
    }

    fn dismiss(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.configured = false;
        self.frame_pending = false;
        // A forget in flight outlives the surface; its answer must not remove a
        // row from the next show's list.
        self.app.pending_forget = None;
        self.ui.set_shown(false);
        // Dropping the layer surface destroys it; there is no hide verb.
        self.disable_ime();
        // wayland-rs only drops a proxy from its map when the destructor is
        // sent, so each per-surface object is torn down explicitly.
        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }
        if let Some(fscale) = self.fscale.take() {
            fscale.destroy();
        }
        if let Some(blur) = self.blur.take() {
            blur.destroy();
        }
        self.blur_rect = None;
        // `ratio` is an output property, not a surface one: keeping it makes the
        // next show allocate at the right size straight away.
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
        if matches!(&item.action, crate::session::Action::None) {
            return;
        }
        let record = serde_json::json!({
            "title": item.title,
            "summary": item.summary,
            "on_click": item.on_click,
            "icon": item.icon,
            "ephemeral": item.ephemeral,
        });
        self.session.select(&record);
        self.session.action(&item.action);
        // The launcher stays up for a beat so the launch does not race the
        // dismiss; a keyboard dismissal is instant.
        self.dismiss_at = Some(Instant::now() + Duration::from_millis(150));
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
                    let rect = (
                        properties.cursor_rect_origin.x as i32,
                        properties.cursor_rect_origin.y as i32,
                        properties.cursor_rect_size.width as i32,
                        properties.cursor_rect_size.height as i32,
                    );
                    let cursor = properties.cursor_position as i32;
                    let state = ImeState {
                        // A zero rectangle means the field is not laid out yet;
                        // sending that parks the candidate window in the corner
                        // of the screen, so the last good one is kept.
                        rect: if rect.2 > 0 && rect.3 > 0 {
                            rect
                        } else {
                            self.ime_state.as_ref().map_or(rect, |old| old.rect)
                        },
                        text: properties.text.to_string(),
                        cursor,
                        anchor: properties
                            .anchor_position
                            .unwrap_or(properties.cursor_position)
                            as i32,
                    };
                    log::debug!(
                        "ime state {state:?} composing={} window={}x{} logical={}x{} sf={}",
                        self.composing,
                        self.width,
                        self.height,
                        self.logical_w,
                        self.logical_h,
                        self.adapter.window().scale_factor()
                    );
                    send_ime_state(ime, &state);
                    self.ime_state = Some(state);
                }
                _ => {}
            }
        }
        let _ = self.conn.flush();
    }

    fn enable_ime(&self) {
        let Some(ime) = self.ime.as_ref() else { return };
        begin_ime(ime);
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
        log::debug!(
            "ime apply {event_type:?} preedit={preedit:?} composing={}",
            self.composing
        );
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
        self.sync_blur();
        if let Err(err) = buffer.attach_to(&surface) {
            log::error!("buffer attach failed: {err}");
            return;
        }
        if let Some(layer) = self.layer.as_ref() {
            layer.commit();
        }
        self.frame_pending = true;
        log::trace!(
            "presented {}x{} (scale {})",
            width,
            height,
            self.effective_scale()
        );
        // The item tree is laid out now, so a fresh focus change carries a real
        // cursor rectangle for the input method.
        if self.refocus_after_frame {
            self.refocus_after_frame = false;
            self.ui.invoke_refocus_input();
        }
    }

    fn apply_size(&mut self) {
        let scale = self.effective_scale();
        self.width = (f64::from(self.logical_w) * scale).round() as u32;
        self.height = (f64::from(self.logical_h) * scale).round() as u32;
        self.adapter
            .window()
            .dispatch_event(WindowEvent::ScaleFactorChanged {
                scale_factor: scale as f32,
            });
        self.adapter
            .window()
            .set_size(slint::PhysicalSize::new(self.width, self.height));
        self.sync_surface_scale();
        self.adapter.window().request_redraw();
    }

    /// The scale a logical pixel is drawn at: the compositor's exact fractional
    /// ratio when it offered one, else the integer buffer scale.
    fn effective_scale(&self) -> f64 {
        buffer_scale(self.scale, self.ratio)
    }

    /// Tell the surface how the buffer maps onto its logical size. With a
    /// fractional ratio the buffer is `logical × ratio` and a viewport maps it
    /// back, so the surface's own buffer scale stays 1; otherwise the integer
    /// scale is what says how big a logical pixel is.
    fn sync_surface_scale(&self) {
        let Some(surface) = self.layer.as_ref().map(WaylandSurface::wl_surface) else {
            return;
        };
        if self.ratio.is_some() {
            surface.set_buffer_scale(1);
            // A zero destination is a protocol error, and this can run before
            // the first configure has given the surface a size.
            if self.logical_w > 0
                && self.logical_h > 0
                && let Some(viewport) = self.viewport.as_ref()
            {
                viewport.set_destination(self.logical_w as i32, self.logical_h as i32);
            }
        } else {
            surface.set_buffer_scale(self.scale.max(1.0) as i32);
        }
    }

    fn rebuild_pool(&mut self) {
        let bytes = (self.width as usize) * (self.height as usize) * 4;
        self.pool = match SlotPool::new(bytes, &self.shm) {
            Ok(pool) => Some(pool),
            Err(err) => {
                log::error!("shared-memory pool failed: {err}");
                None
            }
        };
    }

    /// A `wp_fractional_scale_v1` change: the buffer and the viewport have to be
    /// rebuilt at the new ratio.
    fn set_fractional_scale(&mut self, scale_120: u32) {
        if scale_120 == 0 {
            return;
        }
        let ratio = f64::from(scale_120) / 120.0;
        if self.ratio == Some(ratio) {
            return;
        }
        log::debug!("fractional scale {ratio}");
        self.ratio = Some(ratio);
        if self.visible && self.logical_w > 0 {
            self.apply_size();
            self.rebuild_pool();
        }
    }

    /// The compositor's blur is applied to the card's rounded rect, which is
    /// passed as a middle band plus 2px scanline bands whose inset follows
    /// `r - sqrt(r^2 - (r - dy)^2)`.
    fn sync_blur(&mut self) {
        let Some(effect) = self.blur.as_ref() else {
            return;
        };
        let x = self.ui.get_card_x().round() as i32;
        let y = self.ui.get_card_y().round() as i32;
        let width = self.ui.get_card_width().round() as i32;
        let height = self.ui.get_card_height().round() as i32;
        let radius = self.ui.get_card_radius().round() as i32;
        if width <= 0 || height <= 0 {
            return;
        }
        let snap = |value: i32| value.div_euclid(4) * 4;
        let quantized = (snap(x), snap(y), snap(width), snap(height));
        if self.blur_rect == Some(quantized) {
            return;
        }
        log::debug!("blur region {:?} -> {:?}", (x, y, width, height), quantized);
        self.blur_rect = Some(quantized);
        let Ok(region) = Region::new(&self.compositor) else {
            return;
        };
        for (rx, ry, rw, rh) in blur_rects(x, y, width, height, radius) {
            if rw > 0 && rh > 0 {
                region.add(rx, ry, rw, rh);
            }
        }
        effect.set_blur_region(Some(region.wl_region()));
    }

    fn key(&self, event_type: KeyEventType, keysym: Keysym, repeat: bool) {
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

    fn press_button(&self, position: (f64, f64), button: u32) {
        self.adapter
            .window()
            .dispatch_event(WindowEvent::PointerPressed {
                position: LogicalPosition::new(position.0 as f32, position.1 as f32),
                button: pointer_button(button),
            });
    }

    fn release_button(&self, position: (f64, f64), button: u32) {
        self.adapter
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: LogicalPosition::new(position.0 as f32, position.1 as f32),
                button: pointer_button(button),
            });
    }

    /// The wheel moves the selection, not just the window: a whole row per
    /// notch, with the remainder carried so a trackpad scrolls smoothly. One
    /// notch arrives as both `axis` and `axis_value120`, so only the
    /// high-resolution pair is counted when it is present.
    fn scroll(&mut self, vertical: smithay_client_toolkit::seat::pointer::AxisScroll) {
        let pixels = if vertical.value120 != 0 {
            f64::from(vertical.value120) * 0.5
        } else if vertical.discrete != 0 {
            f64::from(vertical.discrete) * 60.0
        } else {
            vertical.absolute
        };
        let rows = crate::app::whole_rows(&mut self.wheel, pixels, ROW_H);
        if rows != 0 {
            self.app.move_by(rows);
            self.app.publish(&self.ui);
        }
    }
}

/// The card's rounded rect as a small set of rectangles: the input method and
/// the compositor's region API both work in plain rectangles.
fn blur_rects(x: i32, y: i32, width: i32, height: i32, radius: i32) -> Vec<(i32, i32, i32, i32)> {
    let radius = radius.clamp(0, width / 2).clamp(0, height / 2);
    if radius == 0 {
        return vec![(x, y, width, height)];
    }
    let mut rects = Vec::new();
    let mut dy = 0;
    while dy < radius {
        let band = 2.min(radius - dy);
        let falloff = f64::from(radius - dy);
        let inset = (f64::from(radius)
            - (f64::from(radius) * f64::from(radius) - falloff * falloff)
                .max(0.0)
                .sqrt())
        .round() as i32;
        rects.push((x + inset, y + dy, width - 2 * inset, band));
        rects.push((x + inset, y + height - dy - band, width - 2 * inset, band));
        dy += band;
    }
    rects.push((x, y + radius, width, height - 2 * radius));
    rects
}

/// What the input method needs to place its candidates: the caret rectangle and
/// the surrounding text with the cursor in it.
#[derive(Debug, Clone)]
struct ImeState {
    rect: (i32, i32, i32, i32),
    text: String,
    cursor: i32,
    anchor: i32,
}

/// Linux input event codes (`BTN_LEFT` …) as Slint's pointer buttons.
const fn pointer_button(button: u32) -> PointerEventButton {
    match button {
        0x110 => PointerEventButton::Left,
        0x111 => PointerEventButton::Right,
        0x112 => PointerEventButton::Middle,
        0x113 => PointerEventButton::Back,
        0x114 => PointerEventButton::Forward,
        _ => PointerEventButton::Other,
    }
}

/// The scale a logical pixel is drawn at: the exact fractional ratio when the
/// compositor offered one, else the integer buffer scale.
fn buffer_scale(integer: f64, ratio: Option<f64>) -> f64 {
    ratio.unwrap_or_else(|| integer.max(1.0))
}

/// The head of every text-input batch: the content type and the enable.
fn begin_ime(ime: &ZwpTextInputV3) {
    ime.set_content_type(
        zwp_text_input_v3::ContentHint::None,
        zwp_text_input_v3::ContentPurpose::Normal,
    );
    ime.enable();
}

/// `enable` + the caret rectangle + the surrounding text, committed as one
/// double-buffered batch.
fn send_ime_state(ime: &ZwpTextInputV3, state: &ImeState) {
    begin_ime(ime);
    ime.set_cursor_rectangle(state.rect.0, state.rect.1, state.rect.2, state.rect.3);
    ime.set_surrounding_text(state.text.clone(), state.cursor, state.anchor);
    ime.commit();
}

/// The keys that only exist as encoded modifier events, which Slint tracks
/// itself and which must therefore keep flowing during a composition.
const fn is_modifier(keysym: Keysym) -> bool {
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
const fn slint_key(keysym: Keysym) -> Option<Key> {
    let key = match keysym {
        Keysym::BackSpace => Key::Backspace,
        Keysym::Tab => Key::Tab,
        Keysym::Return | Keysym::KP_Enter => Key::Return,
        Keysym::Escape => Key::Escape,
        Keysym::ISO_Left_Tab => Key::Backtab,
        Keysym::Delete => Key::Delete,
        Keysym::Shift_L => Key::Shift,
        Keysym::Shift_R => Key::ShiftR,
        Keysym::Control_L => Key::Control,
        Keysym::Control_R => Key::ControlR,
        Keysym::Alt_L | Keysym::Alt_R => Key::Alt,
        Keysym::ISO_Level3_Shift | Keysym::Mode_switch => Key::AltGr,
        Keysym::Caps_Lock => Key::CapsLock,
        Keysym::Super_L => Key::Meta,
        Keysym::Super_R => Key::MetaR,
        Keysym::Up => Key::UpArrow,
        Keysym::Down => Key::DownArrow,
        Keysym::Left => Key::LeftArrow,
        Keysym::Right => Key::RightArrow,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Page_Up => Key::PageUp,
        Keysym::Page_Down => Key::PageDown,
        Keysym::Insert => Key::Insert,
        _ => return None,
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
    let Some((prefix, _)) = text.split_once(' ') else {
        return "";
    };
    if prefix.len() <= 3 && prefix.chars().all(|c| c.is_ascii_alphabetic()) {
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
        if self.layer.as_ref().map(WaylandSurface::wl_surface) != Some(surface) {
            return;
        }
        // The integer scale is kept even when a fractional ratio is in use: it
        // is the fallback for a compositor that offers the fractional global
        // without the viewporter. `apply_size` picks which one sizes the buffer.
        let scale = f64::from(new_factor.max(1));
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
        if self.layer.as_ref().map(WaylandSurface::wl_surface) == Some(layer.wl_surface()) {
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
        if self.layer.as_ref().map(WaylandSurface::wl_surface) != Some(layer.wl_surface()) {
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
        self.rebuild_pool();
        if self.pool.is_none() {
            return;
        }
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
        if self.layer.as_ref().map(WaylandSurface::wl_surface) == Some(surface) {
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
        if self.layer.as_ref().map(WaylandSurface::wl_surface) == Some(surface) {
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
            if self.layer.as_ref().map(WaylandSurface::wl_surface) != Some(&event.surface) {
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
            zwp_text_input_v3::Event::Enter { surface } => {
                log::debug!("ime enter {surface:?}");
                // The compositor drops geometry sent before `enter`, and a
                // composition changes no text, so nothing else would ever send
                // it again: without this the candidate window sits in the
                // corner of the screen for the whole composition.
                if let (Some(ime), Some(geometry)) = (state.ime.as_ref(), state.ime_state.as_ref())
                {
                    send_ime_state(ime, geometry);
                }
            }
            zwp_text_input_v3::Event::Leave { surface } => {
                log::debug!("ime leave {surface:?}");
            }
            zwp_text_input_v3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                log::debug!("ime preedit {text:?} {cursor_begin}..{cursor_end}");
                // The protocol allows a null string where it means "empty".
                state
                    .pending
                    .preedit(text.unwrap_or_default(), cursor_begin, cursor_end);
            }
            zwp_text_input_v3::Event::CommitString { text } => {
                log::debug!("ime commit {text:?}");
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

impl Dispatch<WpFractionalScaleManagerV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: wp_fractional_scale_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for Shell {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            state.set_fractional_scale(scale);
        }
    }
}

impl Dispatch<WpViewporter, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        log::debug!("background effect capabilities: {event:?}");
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectSurfaceV1,
        _: ext_background_effect_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, whole_rows};

    #[test]
    fn the_keyword_chip_needs_a_space_after_a_short_word() {
        assert_eq!(keyword("b firefox"), "b");
        assert_eq!(keyword("tr hello"), "tr");
        assert_eq!(keyword("toolong x"), "", "four letters is not a prefix");
        assert_eq!(keyword("fire fox"), "", "the first word is too long");
        assert_eq!(keyword("b"), "", "no separator yet");
        assert_eq!(keyword(""), "");
    }

    #[test]
    fn the_fractional_ratio_overrides_the_integer_scale() {
        assert_eq!(buffer_scale(1.0, None), 1.0);
        assert_eq!(
            buffer_scale(2.0, None),
            2.0,
            "the integer ceiling is the fallback"
        );
        assert_eq!(buffer_scale(2.0, Some(1.25)), 1.25, "the exact ratio wins");
        assert_eq!(buffer_scale(0.0, None), 1.0, "a zero scale is clamped");
    }

    #[test]
    fn one_notch_is_one_row_and_the_remainder_carries() {
        let mut carry = 0.0;
        assert_eq!(whole_rows(&mut carry, 64.0, 64.0), 1);
        assert_eq!(whole_rows(&mut carry, 32.0, 64.0), 0);
        assert_eq!(
            whole_rows(&mut carry, 32.0, 64.0),
            1,
            "the two halves make a row"
        );
        assert_eq!(carry, 0.0);
    }

    #[test]
    fn a_direction_change_drops_the_slack() {
        let mut carry = 0.0;
        assert_eq!(whole_rows(&mut carry, -40.0, 64.0), 0);
        assert_eq!(
            whole_rows(&mut carry, 64.0, 64.0),
            1,
            "the first notch back counts"
        );
    }

    #[test]
    fn an_identical_payload_keeps_the_cursor_and_a_new_one_goes_to_the_top() {
        let mut app = App::new();
        let rows = |count: usize| {
            (0..count)
                .map(|i| crate::session::Item {
                    title: format!("row {i}"),
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        };
        assert!(app.set_payload(rows(9), "payload-a".into()));
        app.select(6);
        app.contain();
        assert_eq!((app.selected, app.first_row), (6, 2));

        assert!(
            !app.set_payload(rows(9), "payload-a".into()),
            "a re-send is dropped"
        );
        assert_eq!(
            (app.selected, app.first_row),
            (6, 2),
            "and keeps the cursor"
        );

        assert!(app.set_payload(rows(4), "payload-b".into()));
        assert_eq!(
            (app.selected, app.first_row),
            (0, 0),
            "a new payload starts at the top"
        );
    }

    #[test]
    fn a_forget_removes_the_row_and_keeps_the_selection_in_range() {
        let mut app = App::new();
        let rows = (0..3)
            .map(|i| crate::session::Item {
                title: format!("row {i}"),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        app.set_payload(rows, "p".into());
        app.select(2);
        assert!(app.remove_row_state(2));
        assert_eq!(app.items.len(), 2);
        assert_eq!(app.selected, 1, "the selection follows the shorter list");
        assert!(
            !app.remove_row_state(5),
            "an index outside the list is refused"
        );
    }
}
