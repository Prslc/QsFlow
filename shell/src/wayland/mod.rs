mod ime;
mod input;
mod scale;
mod surface;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use calloop::LoopHandle;
use calloop::channel::Sender;
use calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::background_effect::{BackgroundEffectHandler, BackgroundEffectState};
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::seat::keyboard::Modifiers;
use smithay_client_toolkit::shell::wlr_layer::{LayerShell, LayerSurface};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use tiny_skia::Pixmap;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_shm};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::ZwpTextInputV3;
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

use crate::app::{self, State as Launcher};
use crate::session::backend::{self, BackendEvent};
use crate::session::ipc;
use crate::ui::icons::IconCache;
use crate::ui::text::TextEngine;
use crate::ui::theme;
use crate::wayland::ime::Pending;

/// A changed rectangle in physical buffer pixels. `full` means the whole buffer
/// and ignores the numbers; a zero-size rect means nothing changed. Used both to
/// repaint the retained frame and to keep the two shm buffers in step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Damage {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    full: bool,
}

impl Damage {
    const EMPTY: Damage = Damage {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        full: false,
    };
    const FULL: Damage = Damage {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        full: true,
    };

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Self {
        if w <= 0 || h <= 0 {
            Self::EMPTY
        } else {
            Self {
                x,
                y,
                w,
                h,
                full: false,
            }
        }
    }

    fn is_empty(self) -> bool {
        !self.full && (self.w <= 0 || self.h <= 0)
    }

    fn union(self, other: Self) -> Self {
        if self.full || other.full {
            return Self::FULL;
        }
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }

        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.w).max(other.x + other.w);
        let bottom = (self.y + self.h).max(other.y + other.h);
        Self {
            x,
            y,
            w: right - x,
            h: bottom - y,
            full: false,
        }
    }

    fn add(&mut self, other: Self) {
        *self = self.union(other);
    }

    /// Intersect with a `width`×`height` buffer; `None` when nothing is left.
    /// `full` becomes the whole buffer.
    fn clamped(self, width: u32, height: u32) -> Option<Self> {
        if self.is_empty() {
            return None;
        }
        if self.full {
            return Some(Self::rect(0, 0, width as i32, height as i32));
        }

        let x = self.x.max(0);
        let y = self.y.max(0);
        let right = (self.x + self.w).min(width as i32);
        let bottom = (self.y + self.h).min(height as i32);
        (right > x && bottom > y).then(|| Self::rect(x, y, right - x, bottom - y))
    }
}

pub struct Shell {
    conn: Connection,
    qh: QueueHandle<Shell>,
    loop_handle: LoopHandle<'static, Shell>,
    registry_state: RegistryState,
    compositor_state: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,
    output_state: OutputState,
    seat_state: SeatState,
    effect_state: BackgroundEffectState,
    ime_manager: Option<ZwpTextInputManagerV3>,
    ime: Option<ZwpTextInputV3>,
    /// `wp_fractional_scale_v1`'s manager and the per-surface object: together
    /// with the viewport below, they let a 1.25× output render at 1.25× instead
    /// of the 2× its integer buffer scale asks for.
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    fractional: Option<WpFractionalScaleV1>,
    viewporter: Option<WpViewporter>,
    viewport: Option<WpViewport>,
    /// The destination the viewport was last set to (the surface's logical
    /// size); the request is double-buffered state, so it is sent on change only.
    viewport_destination: Option<(u32, u32)>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    /// Where a worker thread hands back a clipboard read, stamped with the show
    /// it was requested for.
    paste_tx: Sender<(u64, Option<String>)>,
    /// Bumped on every `open`: a paste that outlived its show is dropped.
    paste_generation: u64,
    modifiers: Modifiers,
    keyboard_focus: bool,
    /// The surface exists only while shown: a toggle is create/destroy.
    layer: Option<LayerSurface>,
    /// Whether the compositor has configured the current surface. A layer
    /// surface must commit once with no buffer before it may commit one, and a
    /// payload can arrive from the core inside that window.
    configured: bool,
    effect: Option<ExtBackgroundEffectSurfaceV1>,
    pool: Option<SlotPool>,
    /// Two persistent shm buffers, reused across presents: an unchanged region
    /// is neither copied nor re-uploaded. `buffer_stale[i]` is what the pixmap
    /// has changed since buffer `i` was last written.
    buffers: [Option<Buffer>; 2],
    buffer_stale: [Damage; 2],
    /// The buffer to write next, so the two alternate.
    buffer_next: usize,
    /// Pixmap changes not yet uploaded to any buffer.
    pending_damage: Damage,
    buffer_physical: (u32, u32),
    /// A present was deferred because both buffers were still held by the
    /// compositor; a short timer retries it.
    retry_armed: bool,
    /// The frame as drawn. A blink does not keep a second full-size frame:
    /// it restores the pixels under the caret from `caret_patch` and redraws
    /// the caret alone.
    pixmap: Option<Pixmap>,
    caret_patch: Option<surface::CaretPatch>,
    /// Chosen from what `wl_shm` advertises, at the first present.
    shm_format: wl_shm::Format,
    format_chosen: bool,
    blur_sent: Option<(i32, i32, i32, i32)>,
    ime_cursor_sent: Option<(i32, i32, i32, i32)>,
    wheel_accum: f32,
    pending: Pending,
    icons: IconCache,
    /// Icon specs already sent to the core's `resolve_icon`.
    requested_icons: HashSet<String>,
    text: TextEngine,
    app: Launcher,
    resident: bool,
    timing: bool,
    /// `WAYRUN_IME_LOG=1`: every text-input event and the state `done` left
    /// behind. The preedit path cannot be driven from here (`wtype` never
    /// reaches fcitx5), so a report is only diagnosable from this.
    ime_log: bool,
    started: Instant,
    open_at: Option<Instant>,
    last_present: Option<Instant>,
    first_frame_logged: bool,
    timer_registered: bool,
    /// Whether a `wl_surface.frame` callback is outstanding. Presents are paced
    /// to it — at most one commit per compositor frame — so a burst of input
    /// events cannot leave several buffers in flight and grow the shm pool.
    frame_pending: bool,
    /// A redraw was asked for while a frame callback was outstanding.
    needs_present: bool,
    /// The retained frame has to be laid down whole: a fresh pixmap, a scale or
    /// size change, or the entrance's moving dim. A settled redraw only repaints
    /// the card's rectangle.
    needs_full: bool,
    exit: bool,
}

impl Shell {
    /// The constructor: everything `run()` used to do before it built the state.
    pub fn new(
        conn: &Connection,
        qh: &QueueHandle<Shell>,
        handle: &LoopHandle<'static, Shell>,
        globals: &GlobalList,
        paste_tx: Sender<(u64, Option<String>)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let compositor = CompositorState::bind(globals, qh)?;
        let shm = Shm::bind(globals, qh)?;
        let layer_shell = LayerShell::bind(globals, qh)?;
        let effect_state = BackgroundEffectState::new(globals, qh);
        let ime_manager = ime::bind(globals, qh).ok();
        let fractional = scale::bind(globals, qh);

        Ok(Shell {
            conn: conn.clone(),
            qh: qh.clone(),
            loop_handle: handle.clone(),
            registry_state: RegistryState::new(globals),
            compositor_state: compositor,
            shm,
            layer_shell,
            output_state: OutputState::new(globals, qh),
            seat_state: SeatState::new(globals, qh),
            effect_state,
            ime_manager,
            ime: None,
            fractional_manager: fractional.as_ref().map(|(manager, _)| manager.clone()),
            fractional: None,
            viewporter: fractional.map(|(_, viewporter)| viewporter),
            viewport: None,
            viewport_destination: None,
            keyboard: None,
            pointer: None,
            paste_tx,
            paste_generation: 0,
            modifiers: Modifiers::default(),
            keyboard_focus: false,
            layer: None,
            configured: false,
            effect: None,
            pool: None,
            buffers: [None, None],
            buffer_stale: [Damage::EMPTY, Damage::EMPTY],
            buffer_next: 0,
            pending_damage: Damage::EMPTY,
            buffer_physical: (0, 0),
            retry_armed: false,
            pixmap: None,
            caret_patch: None,
            shm_format: wl_shm::Format::Argb8888,
            format_chosen: false,
            blur_sent: None,
            ime_cursor_sent: None,
            wheel_accum: 0.0,
            pending: Pending::default(),
            icons: IconCache::new(),
            requested_icons: HashSet::new(),
            text: TextEngine::new(),
            app: Launcher::new(),
            resident: std::env::var_os("WAYRUN_RESIDENT").is_some(),
            timing: std::env::var_os("WAYRUN_TIMING").is_some(),
            ime_log: std::env::var_os("WAYRUN_IME_LOG").is_some(),
            started: Instant::now(),
            open_at: None,
            last_present: None,
            first_frame_logged: false,
            timer_registered: false,
            frame_pending: false,
            needs_present: false,
            needs_full: true,
            exit: false,
        })
    }

    // ---- events ------------------------------------------------------------

    pub fn on_backend(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::Theme(config) => self.app.theme = theme::Theme::from_config(&config),
            BackendEvent::Results(items) => {
                let now = Instant::now();
                self.app.apply_results(items, now);
                self.request_icons();
                let size = (30.0 * self.app.scale_factor()).round() as u32;
                let paths: Vec<String> = self
                    .app
                    .rows
                    .iter()
                    .filter_map(|row| row.icon_path.clone())
                    .collect();
                for path in paths {
                    self.icons.warm(&path, size);
                }
            }
            BackendEvent::Icon { spec, path } => {
                // Rasterise before the redraw, like the `Results` arm: the
                // resolve reply arrives while the launcher is up, and a decode
                // on the frame that is being animated is exactly what `warm`
                // exists to avoid.
                if let Some(path) = path.as_deref() {
                    let size = (30.0 * self.app.scale_factor()).round() as u32;
                    self.icons.warm(path, size);
                }
                self.app.set_icon(&spec, path)
            }
            // A confirmed forget is the only thing that removes a row: a
            // provider without `forget` answers `false` and the list is left
            // alone.
            BackendEvent::Forgotten {
                on_click,
                forgotten,
            } => {
                if forgotten {
                    self.app.remove_row(&on_click, Instant::now());
                }
            }
            BackendEvent::CoreExited => self.exit = true,
        }
        self.redraw();
    }

    /// Ask the core for the absolute path of every icon spec that is not one.
    fn request_icons(&mut self) {
        let specs: Vec<String> = self
            .app
            .rows
            .iter()
            .filter_map(|row| row.icon_spec.as_deref())
            .filter(|spec| !spec.starts_with('/'))
            .map(str::to_string)
            .collect();

        for spec in specs {
            if self.app.icon_cache.contains_key(&spec) || !self.requested_icons.insert(spec.clone())
            {
                continue;
            }
            backend::resolve_icon(&spec);
        }
    }

    /// A clipboard read came back from its worker thread: insert it at the
    /// caret and search, like any other query change.
    pub fn on_paste(&mut self, generation: u64, text: Option<String>) {
        let Some(text) = text else { return };
        // The read outlives a dismissal; a paste must not land on the next show.
        if generation != self.paste_generation || self.layer.is_none() {
            return;
        }

        // A single-line field, so a pasted newline would otherwise shape a
        // second line inside it.
        let text: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        self.app.insert(text.trim());
        self.redraw();
        self.query_changed();
    }

    pub fn on_ipc(&mut self, command: ipc::Command) {
        let now = Instant::now();
        match command {
            ipc::Command::Open => self.open(now),
            ipc::Command::Close => self.dismiss(now),
            ipc::Command::Toggle => {
                if self.layer.is_some() {
                    self.dismiss(now);
                } else {
                    self.open(now);
                }
            }
            // answered by the connection thread, never forwarded
            ipc::Command::Status => {}
        }
    }

    /// Ask for a redraw. The commit is paced to the compositor's frame
    /// callback, so a burst of events (a key press plus its release, the
    /// reflow frames, a payload) collapses into one commit per frame instead of
    /// one per event; without that the shm pool doubled under every burst and
    /// never shrank (measured: hundreds of MB while typing).
    pub fn redraw(&mut self) {
        self.needs_present = true;
        self.pump();
        let _ = self.conn.flush();
    }

    /// Present if nothing is outstanding: a frame callback pending means the
    /// compositor has not returned the previous frame yet.
    fn pump(&mut self) {
        if self.frame_pending || self.layer.is_none() || !self.configured {
            return;
        }
        let now = Instant::now();
        if !self.needs_present && !self.app.animating(now) {
            return;
        }
        self.present(now);
    }

    fn arm_timer(&mut self) {
        if self.timer_registered {
            return;
        }
        self.timer_registered = true;
        let _ = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_millis(app::CARET_BLINK_MS)),
            |deadline, _, state: &mut Shell| state.on_timer(deadline),
        );
    }

    /// The caret blink. Nothing else needs a timer: the entrance fade runs on
    /// frame callbacks, and a hidden launcher arms neither.
    fn on_timer(&mut self, now: Instant) -> TimeoutAction {
        if self.layer.is_none() {
            self.timer_registered = false;
            return TimeoutAction::Drop;
        }

        if self.keyboard_focus && self.app.preedit.is_none() {
            // A key press restarts the flash, as Qt's field does: stay on for a
            // whole interval measured from the last edit instead of toggling a
            // free-running timer, which could switch the caret off right after
            // the user typed.
            let phase = now.saturating_duration_since(self.app.caret_at);
            let interval = Duration::from_millis(app::CARET_BLINK_MS);
            if phase < interval {
                self.app.caret_visible = true;
                self.present_caret(now);
                let _ = self.conn.flush();
                return TimeoutAction::ToDuration(interval - phase);
            }

            self.app.caret_visible = !self.app.caret_visible;
            self.present_caret(now);
            let _ = self.conn.flush();
        }

        TimeoutAction::ToDuration(Duration::from_millis(app::CARET_BLINK_MS))
    }

    /// Whether the event loop should stop: set when the core dies or a
    /// non-resident run dismisses.
    pub fn is_done(&self) -> bool {
        self.exit
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

impl ShmHandler for Shell {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl BackgroundEffectHandler for Shell {
    fn background_effect_state(&mut self) -> &mut BackgroundEffectState {
        &mut self.effect_state
    }

    fn update_capabilities(&mut self) {}
}

impl ProvidesRegistryState for Shell {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Shell);
delegate_dispatch2!(Shell);

#[cfg(test)]
mod tests {
    use super::Damage;

    #[test]
    fn damage_unions_and_clamps_to_the_buffer() {
        assert_eq!(Damage::EMPTY.union(Damage::EMPTY), Damage::EMPTY);
        assert_eq!(
            Damage::EMPTY.union(Damage::rect(1, 2, 3, 4)),
            Damage::rect(1, 2, 3, 4)
        );
        assert_eq!(Damage::FULL.union(Damage::rect(1, 2, 3, 4)), Damage::FULL);

        // Two boxes become the box around both.
        let a = Damage::rect(10, 10, 20, 20);
        let b = Damage::rect(40, 5, 10, 40);
        assert_eq!(a.union(b), Damage::rect(10, 5, 40, 40));

        // Clamping trims to the buffer, drops what falls outside, and turns
        // `full` into the whole buffer.
        assert_eq!(
            Damage::rect(-5, -5, 20, 20).clamped(8, 8),
            Some(Damage::rect(0, 0, 8, 8))
        );
        assert_eq!(Damage::rect(100, 100, 5, 5).clamped(8, 8), None);
        assert_eq!(Damage::FULL.clamped(8, 8), Some(Damage::rect(0, 0, 8, 8)));
        assert_eq!(Damage::EMPTY.clamped(8, 8), None);
    }
}
