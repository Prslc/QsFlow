//! Launcher state, messages and subscriptions — the ported behaviour of
//! `ui/SearchWindow.qml` + `ui/SearchBar.qml`.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use iced::animation::{Animation, Easing};
use iced::keyboard::Key;
use iced::keyboard::key::Named;
use iced::time::Instant;
use iced::widget::{Id, operation};
use iced::window::Id as WindowId;
use iced::{Color, Point, Size, Subscription, Task};
use iced_core::input_method;
use iced_exwlshell::reexport::{
    Anchor, BlurOption, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings,
    OutputOption,
};
use iced_exwlshell::shell::{ShellEvent, ShellReceiver};
use iced_exwlshell::to_exwlshell_message;

use crate::backend::{self, BackendEvent};
use crate::geometry;
use crate::ipc::{self, IpcCommand};
use crate::model::ResultItem;
use crate::theme::Theme;

pub const INPUT_ID: Id = Id::new("qsflow-input");
const NAMESPACE: &str = "QsFlow";

#[to_exwlshell_message]
#[derive(Debug, Clone)]
pub enum Message {
    Typed(String),
    Submit,
    Dismiss,
    /// A press inside the card: consumed so it never reaches the dismiss
    /// handler on the backdrop.
    SwallowClick,
    ClearQuery,
    RowClick(usize),
    /// Pointer entered a hover target.
    Hover(Hover),
    /// Pointer left a target: clears the hover only if it is still that one, so
    /// a same-batch `exit(A) enter(B)` cannot wipe B's tint.
    Unhover(Hover),
    /// Whether the pointer is over the card, which gates the wheel so that
    /// scrolling the backdrop does nothing.
    PointerOnCard(bool),
    /// Pointer position, tracked so the hover can be re-derived when the rows
    /// move underneath a stationary pointer.
    PointerMoved(Point),
    PointerLeft,
    /// Mouse wheel over the list, in rows (positive scrolls down).
    Wheel(f32),
    Key(Key, iced::keyboard::Modifiers),
    Ime(input_method::Event),
    Frame(Instant),
    Backend(BackendEvent),
    Ipc(IpcCommand),
    Shell(ShellEvent),
    Resized(WindowId, Size),
    Closed(WindowId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    Row(usize),
    Clear,
}

pub struct Row {
    pub title: String,
    pub summary: Option<String>,
    pub on_click: Option<String>,
    /// Any icon spec (theme name, `papirus:<name>`, absolute path).
    pub icon_spec: Option<String>,
    /// The resolved absolute path, once known.
    pub icon_path: Option<String>,
}

pub struct State {
    pub query: String,
    pub rows: Vec<Row>,
    pub selected: usize,
    /// Index of the top visible row. The list is a fixed five-row window, so it
    /// is scrolled by moving this instead of through a `scroll_to` operation —
    /// see `contain`.
    pub first: usize,
    pub theme: Theme,
    /// The layer surface size in logical pixels. Before the runtime reports the
    /// real one, this holds a fallback: the first frame keeps a full widget tree
    /// (so the text input exists and can be focused) and is invisible anyway —
    /// the entrance animation starts at zero alpha.
    pub surface: Size,
    /// Whether `surface` came from the runtime rather than the fallback.
    surface_known: bool,
    pub hovered: Option<Hover>,
    /// Pointer over the card (see `Message::PointerOnCard`).
    pointer_on_card: bool,
    /// Last pointer position while it is over the surface.
    cursor: Option<Point>,
    /// Wheel deltas smaller than a row (trackpads, the pixel half of a notch).
    wheel_accum: f32,
    /// Last applied result payload: an identical re-send is dropped so the
    /// selection survives it (the QML's `lastResults` dedupe).
    last_payload: String,
    shown: Option<WindowId>,
    /// fcitx can deliver Enter while a preedit is live; Enter over composition
    /// must not launch a row.
    preedit_active: bool,
    resident: bool,
    reduce_motion: bool,
    card_h: Animation<f32>,
    dim: Animation<f32>,
    entrance: Animation<f32>,
    animating: bool,
    dismiss_at: Option<Instant>,
    /// The last blur region sent, so a reflow does not commit one per frame.
    blur_sent: Option<(i32, i32, i32, i32)>,
    /// Spec → path; `None` means "asked, no icon".
    icon_cache: HashMap<String, Option<String>>,
    shell_events: ShellReceiver,
}

/// The card height animation at rest for `rows` rows: the 150ms OutCubic reflow
/// the QML's `Behavior on height` played. `go_mut` retargets it in place.
fn resting_card(rows: usize) -> Animation<f32> {
    Animation::new(geometry::content_h(rows))
        .duration(Duration::from_millis(geometry::REFLOW_MS))
        .easing(Easing::EaseOutCubic)
}

use crate::geometry::MAX_ROWS;

/// Top visible row for `selected`, given the current `first`.
///
/// The row index under a point, `None` outside the card or the list band. The
/// inverse of the layout `view` builds, so it has to agree with `geometry`.
fn row_at(surface: Size, first: usize, rows: usize, point: Point) -> Option<usize> {
    let x = geometry::card_x(surface);
    if point.x < x || point.x > x + geometry::card_w(surface) {
        return None;
    }

    let top = geometry::rows_top(surface);
    if point.y < top || point.y >= top + geometry::list_h(rows) {
        return None;
    }

    let index = first + ((point.y - top) / geometry::ROW_H) as usize;
    (index < rows).then_some(index)
}

/// Whole rows from a possibly fractional wheel delta, carrying the remainder.
/// One mouse notch arrives as a discrete line *and* its pixel equivalent (the
/// compositor sends both `axis` and `axis_value120`), so the pixel half must not
/// add a second row — while a trackpad (pixel deltas only) has to accumulate to
/// one row per ~64px rather than be dropped.
fn take_whole_rows(accum: &mut f32, delta: f32) -> i32 {
    // A reversal starts a new gesture: without this the previous direction's
    // slack (the pixel half of a notch) would swallow the first notch back.
    if accum.signum() * delta.signum() < 0.0 {
        *accum = 0.0;
    }

    *accum += delta;
    let whole = accum.trunc();
    *accum -= whole;
    whole as i32
}

/// The launcher shows at most [`MAX_ROWS`] rows, so the list is a window rather
/// than a scroll offset: `Contain` semantics — move only as far as the selection
/// requires, and never move while it is already visible. (A `scroll_to`
/// operation would be applied by the runtime a dispatch later *without*
/// requesting a redraw, which made a page change visible only on the next blink
/// or keystroke.)
fn contain(selected: usize, first: usize, rows: usize) -> usize {
    if rows <= MAX_ROWS {
        return 0;
    }

    let mut first = first.min(rows - MAX_ROWS);
    if selected < first {
        first = selected;
    }
    if selected >= first + MAX_ROWS {
        first = selected + 1 - MAX_ROWS;
    }
    first
}

/// The runtime events the launcher reacts to, regardless of widget capture.
fn input_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: WindowId,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            Some(Message::Key(key, modifiers))
        }
        iced::Event::InputMethod(event) => Some(Message::Ime(event)),
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Message::PointerMoved(position))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Message::PointerLeft),
        // The launcher has no scrollable any more (the list is a state-driven
        // five-row window), so the wheel has to be handled here.
        iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
            // A downward wheel arrives as a negative y (verified with a
            // virtual pointer), so the message means "rows to move down".
            let rows = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => -y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => -y / crate::geometry::ROW_H,
            };
            (rows != 0.0).then_some(Message::Wheel(rows))
        }
        _ => None,
    }
}

pub fn boot(shell_events: ShellReceiver) -> (State, Task<Message>) {
    let resident = std::env::var("QSFLOW_RESIDENT").is_ok_and(|value| value == "1");
    let reduce_motion = std::env::var("QSFLOW_REDUCED_MOTION").is_ok_and(|value| value == "1");

    let state = State {
        query: String::new(),
        rows: Vec::new(),
        selected: 0,
        first: 0,
        theme: Theme::default(),
        surface: Size::new(1920.0, 1080.0),
        surface_known: false,
        hovered: None,
        pointer_on_card: false,
        cursor: None,
        wheel_accum: 0.0,
        last_payload: String::new(),
        shown: None,
        preedit_active: false,
        resident,
        reduce_motion,
        card_h: resting_card(0),
        dim: Animation::new(0.0),
        entrance: Animation::new(0.0),
        animating: false,
        dismiss_at: None,
        blur_sent: None,
        icon_cache: HashMap::new(),
        shell_events,
    };

    // Non-resident (a dev run) shows the card at boot through the same path the
    // `open` IPC verb takes.
    let task = if resident {
        Task::none()
    } else {
        Task::done(Message::Ipc(IpcCommand::Open))
    };

    (state, task)
}

pub fn subscription(state: &State) -> Subscription<Message> {
    let mut subscriptions = vec![
        // `listen` would drop everything a widget captured — and the focused
        // text input captures Escape, Delete, Enter, Home/End and the left/right
        // arrows. `listen_raw` sees all of them, so the launcher keeps its own
        // Escape/Delete navigation (the arrow keys are untouched by the input
        // and reach us either way).
        iced::event::listen_raw(input_event),
        iced::window::resize_events().map(|(id, size)| Message::Resized(id, size)),
        iced::window::close_events().map(Message::Closed),
        Subscription::run(backend::stream).map(Message::Backend),
        Subscription::run(ipc::stream).map(Message::Ipc),
        state.shell_events.listen().map(Message::Shell),
    ];

    // Frames only while there is work: an idle launcher must not wake at the
    // output refresh rate.
    if state.animating || state.dismiss_at.is_some() {
        subscriptions.push(iced::window::frames().map(Message::Frame));
    }

    Subscription::batch(subscriptions)
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Typed(text) => {
            state.query = text;
            // The core debounces and aborts superseded searches.
            backend::send(&state.query);
            Task::none()
        }
        Message::Submit => {
            if state.preedit_active {
                Task::none()
            } else {
                state.launch()
            }
        }
        Message::Dismiss => state.dismiss(),
        Message::SwallowClick => Task::none(),
        Message::ClearQuery => {
            state.query.clear();
            backend::send("");
            state.selected = 0;
            operation::focus(INPUT_ID)
        }
        Message::RowClick(index) => {
            state.selected = index;
            state.launch()
        }
        Message::Hover(target) => {
            state.hovered = Some(target);
            Task::none()
        }
        Message::Unhover(target) => {
            if state.hovered == Some(target) {
                state.hovered = None;
            }
            Task::none()
        }
        Message::PointerMoved(position) => {
            state.cursor = Some(position);
            Task::none()
        }
        Message::PointerLeft => {
            state.cursor = None;
            Task::none()
        }
        Message::PointerOnCard(over) => {
            state.pointer_on_card = over;
            Task::none()
        }
        Message::Wheel(rows) => {
            if state.pointer_on_card {
                state.scroll(rows);
            }
            Task::none()
        }
        Message::Key(key, _modifiers) => state.key(key),
        Message::Ime(event) => {
            state.ime(event);
            Task::none()
        }
        Message::Frame(now) => state.frame(now),
        Message::Backend(event) => state.backend(event),
        Message::Ipc(IpcCommand::Open) => state.open(),
        Message::Ipc(IpcCommand::Close) => state.dismiss(),
        Message::Ipc(IpcCommand::Toggle) => {
            if state.shown.is_some() {
                state.dismiss()
            } else {
                state.open()
            }
        }
        Message::Shell(ShellEvent::NewShell(info)) => {
            if Some(info.window) != state.shown {
                Task::none()
            } else {
                let id = info.window;
                Task::batch([
                    iced::window::size(id).map(move |size| Message::Resized(id, size)),
                    operation::focus(INPUT_ID),
                ])
            }
        }
        Message::Shell(ShellEvent::Closed(id)) => {
            if Some(id) == state.shown {
                state.surface_gone();
            }
            Task::none()
        }
        // Other shells (the runtime's own surfaces) and output changes are not
        // the launcher's business.
        Message::Shell(_) => Task::none(),
        Message::Resized(id, size) => {
            if Some(id) == state.shown {
                state.resize(size)
            } else {
                Task::none()
            }
        }
        Message::Closed(id) => {
            if Some(id) == state.shown {
                state.surface_gone();
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

impl State {
    /// The card height the layout uses right now.
    pub fn card_height(&self) -> f32 {
        self.card_h.interpolate_with(|value| value, Instant::now())
    }

    pub fn dim_alpha(&self) -> f32 {
        self.dim.interpolate_with(|value| value, Instant::now())
    }

    /// The card's fill alpha: the entrance animation is an opacity fade, since
    /// iced 0.14 exposes no way to scale a widget subtree.
    pub fn entrance_alpha(&self) -> f32 {
        self.entrance
            .interpolate_with(|value| value, Instant::now())
    }

    /// A card-content colour at `alpha`, scaled by the entrance fade. iced has no
    /// subtree opacity, so every colour inside the card goes through here — the
    /// card then arrives as one unit (the QML faded the whole `content` item) and
    /// the first frame really is invisible.
    pub fn fade(&self, color: Color, alpha: f32) -> Color {
        Color {
            a: alpha * self.entrance_alpha(),
            ..color
        }
    }

    fn open(&mut self) -> Task<Message> {
        if self.shown.is_some() {
            return Task::none();
        }

        let (id, task) = Message::layershell_open(NewLayerShellSettings {
            size: LayerSize::FILL,
            layer: Layer::Overlay,
            anchor: Anchor::all(),
            exclusive_zone: Some(-1),
            margin: Some((0, 0, 0, 0)),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            output_option: OutputOption::Active,
            events_transparent: false,
            namespace: Some(NAMESPACE.to_string()),
            // the card rect is unknown until the surface reports its size; the
            // region follows through BlurOptionChange.
            blur_option: BlurOption::None,
        });

        self.shown = Some(id);
        self.query.clear();
        self.selected = 0;
        self.hovered = None;
        self.preedit_active = false;
        self.dismiss_at = None;
        self.blur_sent = None;
        self.contain();
        ipc::VISIBLE.store(true, Ordering::Relaxed);

        // The card opens at its resting height (the QML only animated the
        // height of a *reflow*), so only the backdrop and the card fade play.
        self.card_h = resting_card(self.rows.len());

        if self.reduce_motion {
            self.dim = Animation::new(geometry::DIM_ALPHA);
            self.entrance = Animation::new(1.0);
            self.animating = false;
        } else {
            let now = Instant::now();
            self.dim = Animation::new(0.0)
                .duration(Duration::from_millis(geometry::ENTRANCE_MS))
                .easing(Easing::EaseOutQuint);
            self.entrance = Animation::new(0.0)
                .duration(Duration::from_millis(geometry::ENTRANCE_MS))
                .easing(Easing::EaseOutQuint);
            self.dim.go_mut(geometry::DIM_ALPHA, now);
            self.entrance.go_mut(1.0, now);
            self.animating = true;
        }

        // empty query: usage-ranked history, as the QML's init timer did
        backend::send("");

        task
    }

    /// The runtime destroyed our surface without us asking (the output it was on
    /// went away, or the compositor closed it): drop the state that claimed a
    /// surface exists, including the flag the accept thread answers `status`
    /// from — otherwise `status` keeps reporting `visible` for the session.
    fn surface_gone(&mut self) {
        self.shown = None;
        self.dismiss_at = None;
        self.animating = false;
        self.hovered = None;
        self.preedit_active = false;
        self.blur_sent = None;
        ipc::VISIBLE.store(false, Ordering::Relaxed);
    }

    fn dismiss(&mut self) -> Task<Message> {
        ipc::VISIBLE.store(false, Ordering::Relaxed);

        let task = match (self.shown.take(), self.resident) {
            (Some(id), true) => Task::done(Message::RemoveWindow(id)),
            (Some(_), false) => iced::exit(),
            (None, _) => Task::none(),
        };

        // Dismissal is never animated: Esc and Alt+Space are keyboard
        // interactions and the toggle must stay snappy.
        self.dismiss_at = None;
        self.animating = false;
        self.hovered = None;
        self.preedit_active = false;
        self.blur_sent = None;

        task
    }

    /// `ui/SearchWindow.qml:launch()` — usage recording first, then exactly one
    /// command line to the core.
    fn launch(&mut self) -> Task<Message> {
        let Some(row) = self.rows.get(self.selected) else {
            return Task::none();
        };
        let Some(target) = row.on_click.clone() else {
            return Task::none();
        };

        let usage = serde_json::json!({
            "title": row.title,
            "summary": row.summary.clone().unwrap_or_default(),
            "on_click": target,
            "icon": row.icon_path.clone().unwrap_or_default(),
        });
        backend::send(&format!("select {usage}"));

        if let Some(id) = target.strip_prefix("launch:") {
            backend::send(&format!("launch {id}"));
        } else if let Some(command) = target.strip_prefix("run:") {
            backend::send(&format!("run {command}"));
        } else if let Some(payload) = target.strip_prefix("copy:") {
            backend::send(&format!("copy {payload}"));
        } else if let Some(spec) = target.strip_prefix("action:") {
            backend::send(&format!("action {spec}"));
        } else if target.starts_with("http")
            || target.starts_with("file:")
            || target.starts_with("mailto:")
        {
            backend::send(&format!("open {target}"));
        } else {
            backend::send(&format!("run {target}"));
        }

        self.dismiss_at = Some(Instant::now() + Duration::from_millis(geometry::EXIT_DELAY_MS));
        Task::none()
    }

    /// Delete: drop the selected row from the usage database and from the list.
    fn forget(&mut self) -> Task<Message> {
        let Some(row) = self.rows.get(self.selected) else {
            return Task::none();
        };
        let Some(key) = row.on_click.clone() else {
            return Task::none();
        };

        backend::send(&format!("forget {key}"));
        self.rows.remove(self.selected);
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.contain();
        self.retarget_height(Instant::now())
    }

    fn key(&mut self, key: Key) -> Task<Message> {
        // Home/End are deliberately not intercepted: they belong to the caret.
        match key {
            Key::Named(Named::Escape) => self.dismiss(),
            Key::Named(Named::ArrowUp) => {
                self.selected = self.selected.saturating_sub(1);
                self.contain();
                Task::none()
            }
            Key::Named(Named::ArrowDown) => {
                if self.selected + 1 < self.rows.len() {
                    self.selected += 1;
                }
                self.contain();
                Task::none()
            }
            Key::Named(Named::PageUp) => {
                self.selected = self.selected.saturating_sub(MAX_ROWS);
                self.contain();
                Task::none()
            }
            Key::Named(Named::PageDown) => {
                self.selected = (self.selected + MAX_ROWS).min(self.rows.len().saturating_sub(1));
                self.contain();
                Task::none()
            }
            Key::Named(Named::Delete) => self.forget(),
            _ => Task::none(),
        }
    }

    /// Keep the selection inside the five-row window, minimally: the QML's
    /// `positionViewAtIndex(currentIndex, ListView.Contain)`.
    fn contain(&mut self) {
        self.first = contain(self.selected, self.first, self.rows.len());
        self.resync_hover();
    }

    /// The rows a scroll (or a new payload) puts under a stationary pointer are
    /// *different* rows drawn in the same widgets, so no `enter`/`exit` fires and
    /// the tint would stay on the old row index — at best on the wrong row, at
    /// worst on a row that is no longer drawn at all. Re-derive it from the
    /// tracked pointer instead.
    fn resync_hover(&mut self) {
        let row = self
            .cursor
            .and_then(|point| row_at(self.surface, self.first, self.rows.len(), point));
        if let Some(row) = row {
            self.hovered = Some(Hover::Row(row));
        }
    }

    /// Move the selection by `rows` (the wheel). The selection — not just the
    /// window — has to move: otherwise the accent bar scrolls out of view and
    /// Enter launches a row the user cannot see. The window then follows through
    /// the same `contain` the arrows use, so a wheel notch and an arrow press
    /// are the same gesture.
    fn scroll(&mut self, rows: f32) {
        if self.rows.is_empty() {
            return;
        }

        let whole = take_whole_rows(&mut self.wheel_accum, rows);
        if whole == 0 {
            return;
        }

        let last = (self.rows.len() - 1) as i32;
        self.selected = (self.selected as i32 + whole).clamp(0, last) as usize;
        self.contain();
    }

    fn ime(&mut self, event: input_method::Event) {
        match event {
            input_method::Event::Preedit(text, _) => self.preedit_active = !text.is_empty(),
            input_method::Event::Commit(_) | input_method::Event::Closed => {
                self.preedit_active = false;
            }
            input_method::Event::Opened => {}
        }
    }

    fn resize(&mut self, size: Size) -> Task<Message> {
        self.surface = size;
        self.surface_known = true;
        self.sync_blur()
    }

    fn frame(&mut self, now: Instant) -> Task<Message> {
        if let Some(at) = self.dismiss_at
            && now >= at
        {
            return self.dismiss();
        }

        if self.animating
            && !self.card_h.is_animating(now)
            && !self.dim.is_animating(now)
            && !self.entrance.is_animating(now)
        {
            self.animating = false;
        }

        self.sync_blur()
    }

    fn backend(&mut self, event: BackendEvent) -> Task<Message> {
        match event {
            // the core re-emits the theme when dank-colors.css changes
            BackendEvent::Theme(config) => {
                self.theme = Theme::from_config(&config);
                Task::none()
            }
            BackendEvent::Results(items) => self.apply_results(items),
            BackendEvent::Icon { spec, path } => {
                self.icon_cache.insert(spec.clone(), path.clone());
                for row in &mut self.rows {
                    if row.icon_spec.as_deref() == Some(spec.as_str()) {
                        row.icon_path = path.clone();
                    }
                }
                Task::none()
            }
            BackendEvent::CoreExited => iced::exit(),
        }
    }

    fn apply_results(&mut self, items: Vec<ResultItem>) -> Task<Message> {
        let payload = serde_json::to_string(&items).unwrap_or_default();
        if payload == self.last_payload && !self.rows.is_empty() {
            return Task::none();
        }
        self.last_payload = payload;

        let cache = &self.icon_cache;
        self.rows = items
            .into_iter()
            .map(|item| {
                let icon_spec = item.icon.clone().filter(|spec| !spec.is_empty());
                let icon_path = icon_spec.as_ref().and_then(|spec| {
                    // absolute paths render directly; anything else is a core
                    // resolve_icon round trip
                    if spec.starts_with('/') {
                        Some(spec.clone())
                    } else {
                        cache.get(spec).cloned().flatten()
                    }
                });

                Row {
                    title: item.title,
                    summary: item.summary,
                    on_click: item.on_click,
                    icon_spec,
                    icon_path,
                }
            })
            .collect();

        // a genuinely new payload starts from the top result
        self.selected = 0;
        self.first = 0;
        self.resync_hover();
        let retarget = self.retarget_height(Instant::now());

        let unresolved: Vec<String> = self
            .rows
            .iter()
            .filter_map(|row| row.icon_spec.as_ref())
            .filter(|spec| !spec.starts_with('/') && !self.icon_cache.contains_key(*spec))
            .cloned()
            .collect();
        for spec in unresolved {
            self.icon_cache.insert(spec.clone(), None);
            backend::resolve_icon(&spec);
        }

        retarget
    }

    /// Returns a blur re-sync when the caller must push it itself: with reduced
    /// motion no animation runs, so no `Frame` will arrive to sync it later.
    fn retarget_height(&mut self, now: Instant) -> Task<Message> {
        let target = geometry::content_h(self.rows.len());
        if self.reduce_motion {
            self.card_h = resting_card(self.rows.len());
            return self.sync_blur();
        }

        if self.card_h.value() != target {
            self.card_h.go_mut(target, now);
            // Only ask for frames while a surface exists: a payload landing on a
            // hidden resident launcher must not wake the process at 60fps.
            self.animating |= self.shown.is_some();
        }

        Task::none()
    }

    /// Frost tracks the card: the region is only re-sent when the rounded rect
    /// actually moved.
    fn sync_blur(&mut self) -> Task<Message> {
        let Some(id) = self.shown else {
            return Task::none();
        };
        // the card rect is meaningless until the surface reports its size
        if !self.surface_known {
            return Task::none();
        }

        let height = self.card_height();
        let key = (
            geometry::card_x(self.surface).round() as i32,
            geometry::card_top(self.surface).round() as i32,
            geometry::card_w(self.surface).round() as i32,
            ((height / 4.0).floor() * 4.0) as i32,
        );
        if self.blur_sent == Some(key) {
            return Task::none();
        }
        self.blur_sent = Some(key);

        Task::done(Message::BlurOptionChange {
            id,
            option: BlurOption::Region(geometry::blur_rects(self.surface, height)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{contain, row_at, take_whole_rows};
    use crate::geometry::{self, MAX_ROWS, ROW_H};
    use iced::{Point, Size};

    #[test]
    fn row_at_maps_the_layout_it_is_drawn_in() {
        let surface = Size::new(1920.0, 1080.0);
        let top = geometry::rows_top(surface);
        let x = geometry::card_x(surface) + 10.0;

        // the first row starts where the layout puts it, and rows are ROW_H tall
        assert_eq!(row_at(surface, 0, 20, Point::new(x, top + 1.0)), Some(0));
        assert_eq!(row_at(surface, 0, 20, Point::new(x, top + ROW_H)), Some(1));
        assert_eq!(row_at(surface, 0, 20, Point::new(x, top + 4.9 * ROW_H)), Some(4));
        // the window start is added, and never past the last row
        assert_eq!(row_at(surface, 3, 20, Point::new(x, top + 1.0)), Some(3));
        assert_eq!(row_at(surface, 15, 20, Point::new(x, top + 4.9 * ROW_H)), Some(19));

        // the window's last row ends at the list's bottom edge, and one pixel
        // past it is below the list
        assert_eq!(
            row_at(surface, 0, 20, Point::new(x, top + 5.0 * ROW_H - 1.0)),
            Some(4)
        );
        assert_eq!(row_at(surface, 0, 20, Point::new(x, top + 5.0 * ROW_H)), None);

        // a list shorter than the window cannot be hit below its own last row
        assert_eq!(row_at(surface, 0, 3, Point::new(x, top + 2.5 * ROW_H)), Some(2));
        assert_eq!(row_at(surface, 0, 3, Point::new(x, top + 3.0 * ROW_H)), None);

        // above the card's list, or beside the card: not the pointer's row
        assert_eq!(row_at(surface, 0, 20, Point::new(x, top - 1.0)), None);
        assert_eq!(row_at(surface, 0, 20, Point::new(x - 700.0, top + 1.0)), None);
    }

    #[test]
    fn a_wheel_notch_moves_exactly_one_row() {
        // the line delta and its pixel twin both arrive for one notch
        let mut accum = 0.0;
        assert_eq!(take_whole_rows(&mut accum, 1.0), 1);
        assert_eq!(take_whole_rows(&mut accum, 0.03125), 0);
        assert_eq!(take_whole_rows(&mut accum, 1.0), 1);
        assert_eq!(take_whole_rows(&mut accum, 0.03125), 0);

        // the first notch back moves a row too (the slack must not absorb it)
        assert_eq!(take_whole_rows(&mut accum, -1.0), -1);
        assert_eq!(take_whole_rows(&mut accum, -0.03125), 0);

        // a trackpad sends pixels only: four 16px steps are one row, no fraction
        // is lost
        let mut pixels = 0.0;
        let mut rows = 0;
        for _ in 0..16 {
            rows += take_whole_rows(&mut pixels, 0.25);
        }
        assert_eq!(rows, 4);

        // half a row one way and half back is nothing, but the next half is a row
        let mut trackpad = 0.0;
        assert_eq!(take_whole_rows(&mut trackpad, 0.5), 0);
        assert_eq!(take_whole_rows(&mut trackpad, -0.5), 0);
        assert_eq!(take_whole_rows(&mut trackpad, -0.5), -1);
    }

    #[test]
    fn contain_moves_only_as_far_as_the_selection_needs() {
        // a full first page needs no scroll
        assert_eq!(contain(0, 0, 20), 0);
        assert_eq!(contain(MAX_ROWS - 1, 0, 20), 0);
        // stepping past the window scrolls by exactly one row
        assert_eq!(contain(MAX_ROWS, 0, 20), 1);
        assert_eq!(contain(MAX_ROWS + 1, 1, 20), 2);
        // moving up *inside* the window keeps it put (ListView.Contain) …
        assert_eq!(contain(3, 1, 20), 1);
        // … but leaving it upwards follows the selection
        assert_eq!(contain(0, 3, 20), 0);
        // the window never runs past the last row
        assert_eq!(contain(19, 0, 20), 15);
        assert_eq!(contain(19, 17, 20), 15);
        // a list shorter than the window cannot scroll
        assert_eq!(contain(0, 5, 3), 0);
        assert_eq!(contain(2, 5, 3), 0);
    }
}
