use std::time::Instant;

use cosmic_text::Weight;
use tiny_skia::{
    BlendMode, Color, FillRule, Paint, Path, PathBuilder, Pixmap, Shader, Stroke, Transform,
};

use crate::app::{Hover, State};
use crate::config::AppearanceConfig;
use crate::ui::geom;
use crate::ui::icons::IconCache;
use crate::ui::text::TextEngine;

const TITLE_SIZE: f32 = 14.0;
const SUMMARY_SIZE: f32 = 12.0;
const SUGGESTION_SIZE: f32 = 11.0;
pub const QUERY_SIZE: f32 = 18.0;
pub const ICON_SIZE: f32 = 30.0;
/// The search field's inner insets: the container's 14, the magnifier's 22, the
/// row's 12px spacing and the input's own 8. The IME needs it to place the
/// caret rectangle.
pub const TEXT_INSET: f32 = 14.0 + 22.0 + 12.0 + 8.0;

/// Everything logical→physical scaling goes through here.
struct Canvas {
    scale: f32,
}

impl Canvas {
    fn px(&self, value: f32) -> f32 {
        value * self.scale
    }

    fn color(rgba: [u8; 4]) -> Color {
        Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
    }

    fn paint(rgba: [u8; 4]) -> Paint<'static> {
        Paint {
            shader: Shader::SolidColor(Self::color(rgba)),
            anti_alias: true,
            ..Paint::default()
        }
    }

    fn fill_all(&self, pixmap: &mut Pixmap, rgba: [u8; 4]) {
        if rgba[3] == 0 {
            pixmap.fill(Color::TRANSPARENT);
        } else {
            pixmap.fill(Self::color(rgba));
        }
    }

    /// Overwrite `rect` with the backdrop dim. A region repaint has to put the
    /// backdrop back before drawing over it; `BlendMode::Source` replaces rather
    /// than composites, so the dim is never applied twice.
    fn restore_dim(&self, pixmap: &mut Pixmap, rect: Rect, dim: f32) {
        let Some(path) = round_rect(rect.scaled(self.scale), 0.0) else {
            return;
        };

        let mut paint = Self::paint([0, 0, 0, (dim * 255.0).round() as u8]);
        paint.blend_mode = BlendMode::Source;
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    /// The band below `y` is backdrop. The card's height animates while its
    /// content is already laid out at its final size, so a growing payload
    /// paints rows below the card's edge.
    fn restore_dim_below(&self, pixmap: &mut Pixmap, y: f32, surface: (u32, u32), dim: f32) {
        self.restore_dim(
            pixmap,
            Rect {
                x: 0.0,
                y,
                w: surface.0 as f32,
                h: surface.1 as f32 - y,
            },
            dim,
        );
    }

    fn fill_path(&self, pixmap: &mut Pixmap, path: &Path, rgba: [u8; 4]) {
        if rgba[3] == 0 {
            return;
        }
        pixmap.fill_path(
            path,
            &Self::paint(rgba),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn stroke_round(
        &self,
        pixmap: &mut Pixmap,
        rect: Rect,
        radius: f32,
        width: f32,
        rgba: [u8; 4],
    ) {
        let Some(path) = round_rect(rect.scaled(self.scale), self.px(radius)) else {
            return;
        };
        let stroke = Stroke {
            width: self.px(width),
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &path,
            &Self::paint(rgba),
            &stroke,
            Transform::identity(),
            None,
        );
    }

    /// A rectangle with square corners (the caret, the preedit quad).
    fn fill_rect(&self, pixmap: &mut Pixmap, rect: Rect, rgba: [u8; 4]) {
        self.fill_round(pixmap, rect, 0.0, rgba);
    }

    fn fill_round(&self, pixmap: &mut Pixmap, rect: Rect, radius: f32, rgba: [u8; 4]) {
        if let Some(path) = round_rect(rect.scaled(self.scale), self.px(radius)) {
            self.fill_path(pixmap, &path, rgba);
        }
    }

    fn stroke_line(
        &self,
        pixmap: &mut Pixmap,
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        rgba: [u8; 4],
    ) {
        let mut builder = PathBuilder::new();
        builder.move_to(self.px(from.0), self.px(from.1));
        builder.line_to(self.px(to.0), self.px(to.1));
        let Some(path) = builder.finish() else { return };

        let stroke = Stroke {
            width: self.px(width),
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &path,
            &Self::paint(rgba),
            &stroke,
            Transform::identity(),
            None,
        );
    }

    fn stroke_circle(
        &self,
        pixmap: &mut Pixmap,
        center: (f32, f32),
        radius: f32,
        width: f32,
        rgba: [u8; 4],
    ) {
        let Some(path) =
            PathBuilder::from_circle(self.px(center.0), self.px(center.1), self.px(radius))
        else {
            return;
        };

        let stroke = Stroke {
            width: self.px(width),
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &path,
            &Self::paint(rgba),
            &stroke,
            Transform::identity(),
            None,
        );
    }
}

/// A logical rectangle.
#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn right(self) -> f32 {
        self.x + self.w
    }

    fn bottom(self) -> f32 {
        self.y + self.h
    }

    fn center_y(self) -> f32 {
        self.y + self.h / 2.0
    }

    /// The search field inside a card of top-left `(x, y)` and width `w`: inset
    /// by `PAD`, one `SEARCH_H` tall. The drawn field and the IME's `caret_box`
    /// both build it here so they cannot drift.
    fn field_at(x: f32, y: f32, w: f32) -> Self {
        Self {
            x: x + geom::PAD,
            y: y + geom::PAD,
            w: w - 2.0 * geom::PAD,
            h: geom::SEARCH_H,
        }
    }

    /// The same rectangle in the target's pixels. `round_rect` builds paths in
    /// the pixmap's coordinate space, so every rect handed to it has to be
    /// scaled, not just radii and strokes.
    fn scaled(self, scale: f32) -> Self {
        Self {
            x: self.x * scale,
            y: self.y * scale,
            w: self.w * scale,
            h: self.h * scale,
        }
    }
}

pub fn draw(
    pixmap: &mut Pixmap,
    state: &State,
    text: &mut TextEngine,
    icons: &mut IconCache,
    now: Instant,
    full: bool,
) {
    let timing = std::env::var_os("WAYRUN_TIMING").is_some();
    let mut marks: Vec<(&str, Instant)> = Vec::new();
    let mut mark = |name: &'static str| {
        if timing {
            marks.push((name, Instant::now()));
        }
    };
    mark("start");
    let canvas = Canvas {
        scale: state.scale_factor(),
    };
    let surface = state.surface;
    let theme = state.theme;
    let layout = state.appearance.layout;

    let card = Rect {
        x: layout.card_x(surface),
        y: layout.card_top(surface),
        w: layout.card_w(surface),
        h: state.card_height(now),
    };

    // The frame is retained across presents, so a settled frame only repaints
    // the card's own rectangle and leaves the (unchanged) full-screen dim in
    // place — the dim was the single largest per-frame cost. An animation or a
    // fresh frame repaints the whole surface: the dim is moving, or the
    // backdrop is not there yet.
    let dim = state.dim_alpha(now);
    if full {
        canvas.fill_all(pixmap, [0, 0, 0, (dim * 255.0).round() as u8]);
    } else {
        // A shrink has to erase the old card's rows too, so the region spans the
        // union of the previous and current bottoms, not just the current card.
        let base_bottom = card.bottom().max(state.last_card_bottom);
        canvas.restore_dim(
            pixmap,
            Rect {
                x: card.x,
                y: card.y,
                w: card.w,
                h: base_bottom - card.y,
            },
            dim,
        );
    }
    mark("clear");

    // The card: a translucent fill with a 1px white hairline. The hairline is a
    // *ring*, never a base fill under the whole card, which would raise the
    // interior's composite alpha and cost it its background transmission.
    canvas.fill_round(
        pixmap,
        Rect {
            x: card.x + 1.0,
            y: card.y + 1.0,
            w: card.w - 2.0,
            h: card.h - 2.0,
        },
        layout.radius - 1.0,
        state.fade(theme.container, state.appearance.card_alpha, now),
    );
    canvas.stroke_round(
        pixmap,
        Rect {
            x: card.x + 0.5,
            y: card.y + 0.5,
            w: card.w - 1.0,
            h: card.h - 1.0,
        },
        layout.hairline_radius(),
        1.0,
        state.fade([255, 255, 255], 0.35, now),
    );

    mark("card");
    let field = Rect::field_at(card.x, card.y, card.w);
    canvas.fill_round(
        pixmap,
        field,
        layout.field_radius(),
        state.fade(theme.fg, 0.08, now),
    );

    mark("shapes");

    draw_magnifier(&canvas, pixmap, field, state, now);
    draw_query(&canvas, pixmap, field, state, text, now);
    draw_toolbar(&canvas, pixmap, field, state, text, now);
    mark("query");

    if !state.rows.is_empty() {
        draw_list(&canvas, pixmap, surface, state, text, icons, now);
    }
    mark("list");

    draw_footer(&canvas, pixmap, surface, state, text, now);
    mark("footer");

    // A payload that grows the card lays its rows out immediately while the
    // height still animates: the band below the card's current bottom edge
    // belongs to the backdrop, not to the card.
    let resting = layout.card_top(surface) + layout.content_h(state.rows.len());
    let bottom = card.y + card.h;
    let mut band = None;
    if full && bottom < resting {
        canvas.restore_dim_below(pixmap, bottom, surface, dim);
        band = Some(bottom);
    }

    if timing && marks.len() > 1 {
        let mut previous = marks[0].1;
        let mut line = String::from("wayrun: draw");
        for (name, at) in &marks[1..] {
            line.push_str(&format!(" {}={:?}", name, at.duration_since(previous)));
            previous = *at;
        }
        line.push_str(&format!(
            " total={:?}",
            marks[marks.len() - 1].1.duration_since(marks[0].1)
        ));
        if let Some(bottom) = band {
            line.push_str(&format!(" reflow_band_from={bottom}"));
        }
        eprintln!("{line}");
    }
}

fn draw_magnifier(canvas: &Canvas, pixmap: &mut Pixmap, field: Rect, state: &State, now: Instant) {
    // A 2px circle of radius 6.2 at (9, 9) with a handle to (19, 19) in a 22×22
    // box, drawn rather than loaded.
    let x = field.x + 14.0;
    let y = field.center_y() - 11.0;
    let color = state.fade(state.theme.fg, 0.55, now);

    canvas.stroke_circle(pixmap, (x + 9.0, y + 9.0), 6.2, 2.0, color);
    canvas.stroke_line(
        pixmap,
        (x + 13.8, y + 13.8),
        (x + 19.0, y + 19.0),
        2.0,
        color,
    );
}

/// The field's text area: its left edge and its width. The field is
/// single-line, so a wider query is scrolled inside the box rather than clipped
/// with the caret outside it.
fn text_area(field: Rect) -> (f32, f32) {
    (field.x + TEXT_INSET, field.w - TEXT_INSET - 40.0)
}

/// How far the query has to be scrolled left for a caret at `caret_x` to stay
/// inside the text area.
fn scroll_for(caret_x: f32, area: (f32, f32)) -> f32 {
    (caret_x + 2.0 - (area.0 + area.1)).max(0.0)
}

fn draw_query(
    canvas: &Canvas,
    pixmap: &mut Pixmap,
    field: Rect,
    state: &State,
    text: &mut TextEngine,
    now: Instant,
) {
    let area = text_area(field);
    let (x, width) = area;
    let size = QUERY_SIZE * canvas.scale;
    let (fg, hint) = (state.theme.fg, state.theme.fg);
    let clip = [
        canvas.px(x),
        canvas.px(field.y),
        canvas.px(width),
        canvas.px(field.h),
    ];

    let placeholder = state.query.is_empty() && state.preedit.is_none();
    if placeholder {
        let shaped = text.shape("Search apps, files, web...", size, Weight::MEDIUM);
        let top = field.center_y() - shaped.height / (2.0 * canvas.scale);
        text.draw(
            pixmap,
            &shaped,
            state.fade(hint, 0.55, now),
            canvas.px(x),
            canvas.px(top),
            Some(clip),
        );
        return;
    }

    let shaped = text.shape(&state.query, size, Weight::MEDIUM);
    let top = field.center_y() - shaped.height / (2.0 * canvas.scale);
    // the drawn caret and the rectangle the IME is told about are one geometry
    let (caret, shift) = caret_box(state, text);

    // A keyboard selection sits under the glyphs at the accent's 35%, clipped to
    // the same text area.
    if let Some((start, end)) = state.selection() {
        let from = text.shape(&state.query[..start], size, Weight::MEDIUM);
        let to = text.shape(&state.query[..end], size, Weight::MEDIUM);
        let left = (x + from.width / canvas.scale - shift).max(x);
        let right = (x + to.width / canvas.scale - shift).min(x + width);
        if right > left {
            canvas.fill_rect(
                pixmap,
                Rect {
                    x: left,
                    y: caret.y,
                    w: right - left,
                    h: caret.h,
                },
                state.fade(state.theme.primary, 0.35, now),
            );
        }
    }

    text.draw(
        pixmap,
        &shaped,
        state.fade(fg, 1.0, now),
        canvas.px(x - shift),
        canvas.px(top),
        Some(clip),
    );

    // The preedit is drawn at the caret, never inserted into the query: glyphs
    // in the text colour over a translucent selection quad, so a composition
    // stays legible on a transparent surface.
    if let Some(preedit) = &state.preedit {
        let shaped = text.shape(preedit, size, Weight::MEDIUM);
        let quad = Rect {
            x: caret.x,
            y: field.center_y() - 14.0,
            // a long composition is cut at the field's edge, not painted over
            // the toolbar
            w: (x + width - caret.x).clamp(0.0, shaped.width / canvas.scale + 1.0),
            h: 28.0,
        };
        canvas.fill_rect(pixmap, quad, state.fade(fg, 0.25, now));
        text.draw(
            pixmap,
            &shaped,
            state.fade(fg, 1.0, now),
            canvas.px(caret.x),
            canvas.px(top),
            Some(clip),
        );
    }
}

/// The caret's box in logical units, and how far the query is scrolled so it
/// fits the field. The one place the caret geometry is computed: the drawn
/// caret and the rectangle the IME is told about must not drift apart.
fn caret_box(state: &State, text: &mut TextEngine) -> (Rect, f32) {
    let surface = state.surface;
    let layout = state.appearance.layout;
    let field = Rect::field_at(
        layout.card_x(surface),
        layout.card_top(surface),
        layout.card_w(surface),
    );
    let scale = state.scale_factor();
    let before = text.shape(state.before_caret(), QUERY_SIZE * scale, Weight::MEDIUM);
    let area = text_area(field);
    let shift = scroll_for(area.0 + before.width / scale, area);

    // The caret is 1px wide at the *line box* height, not at the font size.
    let line_height = (QUERY_SIZE * crate::ui::text::LINE_HEIGHT).round();
    (
        Rect {
            x: area.0 + before.width / scale - shift,
            y: field.center_y() - line_height / 2.0,
            w: 1.0,
            h: line_height,
        },
        shift,
    )
}

/// The caret as the IME's `set_cursor_rectangle` wants it: surface-local
/// logical coordinates, so it follows the same scroll as the drawn caret.
pub fn caret_rect(state: &State, text: &mut TextEngine) -> (i32, i32, i32, i32) {
    let (caret, _) = caret_box(state, text);
    (
        caret.x.round() as i32,
        caret.y.round() as i32,
        (caret.w.round() as i32).max(1),
        caret.h.round() as i32,
    )
}

/// The caret, drawn in its own pass: it is the only thing that changes on a
/// blink, and a blink must not re-render the whole surface.
pub fn draw_caret(pixmap: &mut Pixmap, state: &State, text: &mut TextEngine, now: Instant) {
    if !state.caret_visible || state.preedit.is_some() {
        return;
    }

    let canvas = Canvas {
        scale: state.scale_factor(),
    };
    let (caret, _) = caret_box(state, text);
    canvas.fill_rect(pixmap, caret, state.fade(state.theme.fg, 1.0, now));
}

fn draw_toolbar(
    canvas: &Canvas,
    pixmap: &mut Pixmap,
    field: Rect,
    state: &State,
    text: &mut TextEngine,
    now: Instant,
) {
    let font = SUGGESTION_SIZE * canvas.scale;
    let mut right = field.right() - 8.0;

    if !state.query.is_empty() {
        let circle = Rect {
            x: right - 26.0,
            y: field.center_y() - 13.0,
            w: 26.0,
            h: 26.0,
        };
        let hovered = state.hovered == Some(Hover::Clear);
        canvas.fill_round(
            pixmap,
            circle,
            13.0,
            state.fade(state.theme.fg, if hovered { 0.18 } else { 0.10 }, now),
        );

        let shaped = text.shape("✕", 12.0 * canvas.scale, Weight::NORMAL);
        // the ✕'s ink sits above and left of its line box; these shifts put the
        // centred box back on the circle
        text.draw(
            pixmap,
            &shaped,
            state.fade(state.theme.fg, 0.55, now),
            canvas.px(circle.x + 13.0) - shaped.width / 2.0 + 0.5,
            canvas.px(circle.center_y()) - shaped.height / 2.0 + canvas.px(1.5),
            None,
        );
        right = circle.x - 6.0;
    }

    let Some(prefix) = state.keyword_prefix() else {
        return;
    };
    let shaped = text.shape(prefix, font, Weight::BOLD);
    let chip = Rect {
        x: right - shaped.width / canvas.scale - 16.0,
        y: field.center_y() - 12.0,
        w: shaped.width / canvas.scale + 16.0,
        h: 24.0,
    };
    canvas.fill_round(
        pixmap,
        chip,
        state.appearance.layout.chip_radius(),
        state.fade(state.theme.primary, 0.16, now),
    );
    text.draw(
        pixmap,
        &shaped,
        state.fade(state.theme.primary, 1.0, now),
        canvas.px(chip.x) + canvas.px(8.0),
        canvas.px(chip.center_y()) - shaped.height / 2.0,
        None,
    );
}

fn draw_list(
    canvas: &Canvas,
    pixmap: &mut Pixmap,
    surface: (u32, u32),
    state: &State,
    text: &mut TextEngine,
    icons: &mut IconCache,
    now: Instant,
) {
    let theme = state.theme;
    let layout = state.appearance.layout;
    let left = layout.card_x(surface) + geom::PAD;
    let width = layout.card_w(surface) - 2.0 * geom::PAD;
    let top = layout.rows_top(surface);

    for (index, row) in state
        .rows
        .iter()
        .enumerate()
        .skip(state.first)
        .take(layout.max_rows)
    {
        let y = top + (index - state.first) as f32 * geom::ROW_H;
        let rect = Rect {
            x: left,
            y,
            w: width,
            h: geom::ROW_H,
        };

        let selected = index == state.selected;
        let hovered = state.hovered == Some(Hover::Row(index));
        let background = match (selected, hovered) {
            (true, _) => Some(state.fade(theme.primary, 0.15, now)),
            (false, true) => Some(state.fade(theme.primary, 0.08, now)),
            (false, false) => None,
        };
        if let Some(background) = background {
            canvas.fill_round(pixmap, rect, layout.row_radius(), background);
        }

        // Always 3px wide so the icon sits at the same x on every row; only the
        // selected row paints it.
        if selected {
            canvas.fill_round(
                pixmap,
                Rect {
                    x: rect.x + 3.0,
                    y: rect.center_y() - 14.0,
                    w: 3.0,
                    h: 28.0,
                },
                1.5,
                state.fade(theme.primary, 1.0, now),
            );
        }

        let icon_x = rect.x + 11.0;
        if let Some(path) = row.icon.as_deref() {
            icons.draw(
                pixmap,
                path,
                canvas.px(icon_x),
                canvas.px(rect.center_y() - ICON_SIZE / 2.0),
                (ICON_SIZE * canvas.scale).round() as u32,
                state.entrance(now),
            );
        }

        let labels_x = icon_x + ICON_SIZE + 12.0;
        // The selected row's ↵ hint is part of the layout: the labels must
        // leave room for it, so a long title cannot run underneath it.
        let enter = selected.then(|| text.shape("↵", 13.0 * canvas.scale, Weight::NORMAL));
        let enter_w = enter
            .as_ref()
            .map_or(0.0, |shaped| shaped.width / canvas.scale + 12.0);
        let labels_max = (rect.right() - 10.0 - labels_x - enter_w).max(0.0);

        let title = text.fit(
            &row.title,
            TITLE_SIZE * canvas.scale,
            Weight::BOLD,
            labels_max * canvas.scale,
        );
        let title_h = title.height / canvas.scale;
        let summary = row.summary.as_deref().map(|summary| {
            text.fit(
                summary,
                SUMMARY_SIZE * canvas.scale,
                Weight::NORMAL,
                labels_max * canvas.scale,
            )
        });
        let summary_h = summary
            .as_ref()
            .map_or(0.0, |shaped| shaped.height / canvas.scale);

        // The labels column holds title + 2px + summary, centred in the row.
        let block = title_h
            + if summary.is_some() {
                2.0 + summary_h
            } else {
                0.0
            };
        let labels_top = rect.center_y() - block / 2.0;
        let clip = [
            canvas.px(labels_x),
            canvas.px(rect.y),
            canvas.px(labels_max),
            canvas.px(rect.h),
        ];

        text.draw(
            pixmap,
            &title,
            state.fade(theme.fg, 1.0, now),
            canvas.px(labels_x),
            canvas.px(labels_top),
            Some(clip),
        );
        if let Some(summary) = &summary {
            text.draw(
                pixmap,
                summary,
                state.fade(theme.fg, 0.7, now),
                canvas.px(labels_x),
                canvas.px(labels_top + title_h + 2.0),
                Some(clip),
            );
        }

        if let Some(check) = &enter {
            text.draw(
                pixmap,
                check,
                state.fade(theme.primary, 0.55, now),
                canvas.px(rect.right() - 10.0) - check.width,
                canvas.px(rect.center_y()) - check.height / 2.0,
                None,
            );
        }
    }
}

/// The footer's left-hand hint: the launch keys once there are rows, a distinct
/// "No results" when a non-empty query came back empty, and the history help
/// otherwise. `No results` must not be confused with the untouched empty state,
/// or a searched-but-empty payload reads as "not searched".
fn footer_hint(rows: usize, query_empty: bool) -> &'static str {
    if rows > 0 {
        "↵ Launch   ↑↓ Move   ⌫ Forget   Esc Close"
    } else if query_empty {
        "Type ? for help"
    } else {
        "No results"
    }
}

fn draw_footer(
    canvas: &Canvas,
    pixmap: &mut Pixmap,
    surface: (u32, u32),
    state: &State,
    text: &mut TextEngine,
    now: Instant,
) {
    let empty = state.rows.is_empty();
    let no_match = empty && !state.query.is_empty();
    let hints = footer_hint(state.rows.len(), state.query.is_empty());
    let count = if empty {
        String::new()
    } else {
        format!("{} results", state.rows.len())
    };

    // The footer is the last band of the card, derived from the same height the
    // card itself animates to.
    let layout = state.appearance.layout;
    let y =
        layout.card_top(surface) + layout.content_h(state.rows.len()) - geom::PAD - geom::FOOTER_H;
    let size = SUGGESTION_SIZE * canvas.scale;
    let left = layout.card_x(surface) + geom::PAD;

    let shaped = text.shape(hints, size, Weight::NORMAL);
    text.draw(
        pixmap,
        &shaped,
        state.fade(
            if no_match {
                state.theme.primary
            } else {
                state.theme.fg
            },
            if no_match { 0.7 } else { 0.5 },
            now,
        ),
        canvas.px(left),
        canvas.px(y + (geom::FOOTER_H - shaped.height / canvas.scale) / 2.0),
        None,
    );

    if !count.is_empty() {
        let shaped = text.shape(&count, size, Weight::NORMAL);
        text.draw(
            pixmap,
            &shaped,
            state.fade(state.theme.fg, 0.45, now),
            canvas.px(layout.card_x(surface) + layout.card_w(surface) - geom::PAD) - shaped.width,
            canvas.px(y + (geom::FOOTER_H - shaped.height / canvas.scale) / 2.0),
            None,
        );
    }
}

/// A rounded rectangle as four quadratic-cornered cubic arcs (kappa).
fn round_rect(rect: Rect, radius: f32) -> Option<Path> {
    let r = radius.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
    // The standard circular-arc kappa for a 90° cubic approximation.
    let k = r * 0.552_284_8;
    let (x, y, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());

    let mut builder = PathBuilder::new();
    builder.move_to(x + r, y);
    builder.line_to(right - r, y);
    builder.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    builder.line_to(right, bottom - r);
    builder.cubic_to(
        right,
        bottom - r + k,
        right - r + k,
        bottom,
        right - r,
        bottom,
    );
    builder.line_to(x + r, bottom);
    builder.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    builder.line_to(x, y + r);
    builder.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    builder.close();
    builder.finish()
}

/// A micro-benchmark for the software rasteriser, run by `wayrun bench`.
pub fn bench() {
    let mut pixmap = Pixmap::new(1920, 1080).unwrap();
    let repeats = 20;

    let at = Instant::now();
    for _ in 0..repeats {
        pixmap.fill(Color::from_rgba8(
            0,
            0,
            0,
            (AppearanceConfig::default().dim_alpha * 255.0).round() as u8,
        ));
    }
    println!("pixmap.fill (2.07M px): {:?}/frame", at.elapsed() / repeats);

    let square = Rect {
        x: 595.0,
        y: 302.0,
        w: 730.0,
        h: 460.0,
    };
    let square_path = round_rect(square, 0.0).unwrap();
    let paint = Canvas::paint([36, 40, 59, 184]);
    let at = Instant::now();
    for _ in 0..repeats {
        pixmap.fill_path(
            &square_path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    println!(
        "fill_path square  {}x{}: {:?}/frame",
        square.w,
        square.h,
        at.elapsed() / repeats
    );

    let round_path = round_rect(square, 16.0).unwrap();
    let at = Instant::now();
    for _ in 0..repeats {
        pixmap.fill_path(
            &round_path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    println!(
        "fill_path rounded {}x{}: {:?}/frame",
        square.w,
        square.h,
        at.elapsed() / repeats
    );

    let tiny = Rect {
        x: 610.0,
        y: 316.0,
        w: 700.0,
        h: 52.0,
    };
    let tiny_path = round_rect(tiny, 9.0).unwrap();
    let at = Instant::now();
    for _ in 0..repeats {
        pixmap.fill_path(
            &tiny_path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    println!(
        "fill_path field   {}x{}: {:?}/frame",
        tiny.w,
        tiny.h,
        at.elapsed() / repeats
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap() -> Pixmap {
        Pixmap::new(64, 64).unwrap()
    }

    /// The 8-bit alpha the dim writes, derived from the one constant.
    fn dim_u8() -> u8 {
        (AppearanceConfig::default().dim_alpha * 255.0).round() as u8
    }

    #[test]
    fn a_long_query_scrolls_so_the_caret_stays_in_the_field() {
        let field = Rect {
            x: 100.0,
            y: 10.0,
            w: 532.0,
            h: 52.0,
        };
        let area = text_area(field);

        // a caret inside the area does not scroll the query
        assert_eq!(scroll_for(area.0 + 10.0, area), 0.0);
        // one past the right edge scrolls by exactly the overflow, leaving the
        // caret two logical pixels inside the area
        let caret = area.0 + area.1 + 5.0;
        assert_eq!(caret - scroll_for(caret, area), area.0 + area.1 - 2.0);
    }

    #[test]
    fn the_footer_separates_no_results_from_an_untouched_field() {
        // an empty field is the history view, not a failed search
        assert_eq!(footer_hint(0, true), "Type ? for help");
        assert_eq!(footer_hint(0, false), "No results");
        assert!(footer_hint(3, false).starts_with("↵ Launch"));
    }

    #[test]
    fn a_reflow_puts_the_band_below_the_card_back_to_the_backdrop() {
        let canvas = Canvas { scale: 1.0 };
        let mut pixmap = pixmap();

        // the "content" the card is still growing over
        canvas.fill_all(&mut pixmap, [255, 255, 255, 255]);
        assert_eq!(pixmap.pixel(32, 40).unwrap().alpha(), 255);

        // 0.30 dim over transparent, the same value the backdrop has
        canvas.restore_dim_below(
            &mut pixmap,
            32.0,
            (64, 64),
            AppearanceConfig::default().dim_alpha,
        );

        let below = pixmap.pixel(32, 40).unwrap();
        assert_eq!(
            below.alpha(),
            dim_u8(),
            "the band is the dim, not the content"
        );
        assert_eq!((below.red(), below.green(), below.blue()), (0, 0, 0));
        // and everything above the band is untouched
        assert_eq!(pixmap.pixel(32, 31).unwrap().alpha(), 255);
    }

    #[test]
    fn geometry_is_scaled_into_the_buffer_not_only_radii_and_strokes() {
        let canvas = Canvas { scale: 2.0 };
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        canvas.fill_all(&mut pixmap, [0, 0, 0, 0]);

        // a 10x10 logical rect at the origin covers the first 20x20 buffer pixels
        canvas.fill_round(
            &mut pixmap,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            0.0,
            [255, 255, 255, 255],
        );
        assert_eq!(pixmap.pixel(19, 19).unwrap().alpha(), 255, "inside");
        assert_eq!(pixmap.pixel(21, 21).unwrap().alpha(), 0, "outside");

        // and a rect at logical (10, 10) starts at buffer (20, 20)
        canvas.fill_round(
            &mut pixmap,
            Rect {
                x: 10.0,
                y: 10.0,
                w: 5.0,
                h: 5.0,
            },
            0.0,
            [255, 255, 255, 255],
        );
        assert_eq!(pixmap.pixel(20, 20).unwrap().alpha(), 255, "second rect");
        assert_eq!(pixmap.pixel(9, 9).unwrap().alpha(), 255, "first rect stays");
    }

    #[test]
    fn the_reflow_band_is_scaled_like_everything_else() {
        let canvas = Canvas { scale: 2.0 };
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        canvas.fill_all(&mut pixmap, [255, 255, 255, 255]);

        // the band starts at logical 10, i.e. buffer row 20
        canvas.restore_dim_below(
            &mut pixmap,
            10.0,
            (64, 64),
            AppearanceConfig::default().dim_alpha,
        );
        assert_eq!(pixmap.pixel(32, 19).unwrap().alpha(), 255, "above the band");
        assert_eq!(
            pixmap.pixel(32, 20).unwrap().alpha(),
            dim_u8(),
            "first band row"
        );
        assert_eq!(
            pixmap.pixel(32, 63).unwrap().alpha(),
            dim_u8(),
            "last band row"
        );
    }

    #[test]
    fn scale_one_leaves_the_geometry_alone() {
        let canvas = Canvas { scale: 1.0 };
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        canvas.fill_all(&mut pixmap, [0, 0, 0, 0]);
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 5.0,
            h: 5.0,
        };
        assert_eq!(rect.scaled(1.0).x, 10.0, "scaled(1.0) is the identity");
        assert_eq!(rect.scaled(1.0).w, 5.0);
        canvas.fill_round(&mut pixmap, rect, 0.0, [255, 255, 255, 255]);
        assert_eq!(
            pixmap.pixel(10, 10).unwrap().alpha(),
            255,
            "starts at 10,10"
        );
        assert_eq!(
            pixmap.pixel(15, 15).unwrap().alpha(),
            0,
            "ends before 15,15"
        );
    }

    #[test]
    fn the_base_fill_is_the_dim_with_no_clear_under_it() {
        let mut state = State::new();
        state.surface = (64, 64);
        // Start at the settled end state so the dim is at full strength.
        state.reduce_motion = true;

        let mut pixmap = Pixmap::new(64, 64).unwrap();
        let mut text = TextEngine::new();
        let mut icons = IconCache::new();
        draw(
            &mut pixmap,
            &state,
            &mut text,
            &mut icons,
            Instant::now(),
            true,
        );

        // Above the card only the dim exists; a clear under the dim would show
        // up as alpha 0.
        let pixel = pixmap.pixel(0, 0).unwrap();
        assert_eq!(pixel.alpha(), dim_u8(), "the dim is the base");
        assert_eq!((pixel.red(), pixel.green(), pixel.blue()), (0, 0, 0));
    }

    #[test]
    fn a_settled_repaint_only_touches_the_card_rectangle() {
        let mut state = State::new();
        state.surface = (1600, 1080);
        state.reduce_motion = true;

        let mut text = TextEngine::new();
        let mut icons = IconCache::new();
        let mut pixmap = Pixmap::new(1600, 1080).unwrap();
        // A sentinel everywhere: a region repaint must leave the backdrop alone.
        pixmap.fill(Color::from_rgba8(255, 0, 255, 255));

        draw(
            &mut pixmap,
            &state,
            &mut text,
            &mut icons,
            Instant::now(),
            false,
        );

        // A point outside the card keeps the sentinel untouched.
        let outside = pixmap.pixel(0, 0).unwrap();
        assert_eq!((outside.red(), outside.blue()), (255, 255), "outside");

        // A point inside the card was restored to the dim and painted over, so
        // it can no longer be the sentinel.
        let inside = pixmap
            .pixel(
                (state.appearance.layout.card_x(state.surface)
                    + state.appearance.layout.card_w(state.surface) / 2.0) as u32,
                (state.appearance.layout.card_top(state.surface) + 10.0) as u32,
            )
            .unwrap();
        assert_ne!((inside.red(), inside.green(), inside.blue()), (255, 0, 255));
    }

    #[test]
    fn a_settled_shrink_erases_the_old_cards_rows() {
        let mut state = State::new();
        state.surface = (1600, 1080);
        state.reduce_motion = true;
        let card_h = state.appearance.layout.content_h(0);
        // The previous card was 200px taller than the current one.
        state.last_card_bottom = state.appearance.layout.card_top(state.surface) + card_h + 200.0;

        let mut text = TextEngine::new();
        let mut icons = IconCache::new();
        let mut pixmap = Pixmap::new(1600, 1080).unwrap();
        pixmap.fill(Color::from_rgba8(255, 0, 255, 255));

        draw(
            &mut pixmap,
            &state,
            &mut text,
            &mut icons,
            Instant::now(),
            false,
        );

        // Below the current card but inside the old one: the dim, not the
        // sentinel and not a leftover row.
        let y = (state.appearance.layout.card_top(state.surface) + card_h + 100.0) as u32;
        let pixel = pixmap.pixel(800, y).unwrap();
        assert_ne!(
            (pixel.red(), pixel.green(), pixel.blue()),
            (255, 0, 255),
            "the old card's rows were erased"
        );
    }

    #[test]
    fn the_reflow_band_starts_at_the_card_edge() {
        let canvas = Canvas { scale: 1.0 };
        let mut pixmap = pixmap();
        canvas.fill_all(&mut pixmap, [255, 255, 255, 255]);
        canvas.restore_dim_below(
            &mut pixmap,
            0.0,
            (64, 64),
            AppearanceConfig::default().dim_alpha,
        );
        // every row is the dim, including the first one
        for y in [0, 1, 63] {
            assert_eq!(pixmap.pixel(32, y).unwrap().alpha(), dim_u8(), "y={y}");
        }
    }
}
