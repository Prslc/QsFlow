use std::time::Instant;

use cosmic_text::Weight;
use tiny_skia::Pixmap;

use crate::app::State;
use crate::ui::geom;
use crate::ui::text::TextEngine;

use super::canvas::{Canvas, SUGGESTION_SIZE};

/// The footer's left hint: the panel's keys when open, the launch keys once rows
/// exist, a distinct "No results" for an empty search, else history help.
pub(super) fn footer_hint(rows: usize, query_empty: bool, panel: bool) -> &'static str {
    if panel {
        "↵ Run   ↑↓ Move   Esc Back"
    } else if rows > 0 {
        "↵ Launch   Shift+↵ Actions   ↑↓ Move   Esc Close"
    } else if query_empty {
        "Type ? for help"
    } else {
        "No results"
    }
}

pub(super) fn draw_footer(
    canvas: &Canvas,
    pixmap: &mut Pixmap,
    surface: (u32, u32),
    state: &State,
    text: &mut TextEngine,
    now: Instant,
) {
    let panel = state.menu.is_some();
    let empty = state.rows.is_empty();
    let no_match = empty && !state.query.is_empty();
    let hints = footer_hint(state.rows.len(), state.query.is_empty(), panel);
    let count = if panel {
        format!(
            "{} actions",
            state.menu.as_ref().map_or(0, |menu| menu.actions.len())
        )
    } else if empty {
        String::new()
    } else {
        format!("{} results", state.rows.len())
    };

    // The footer is the last band of the card, derived from the same height the
    // card itself animates to.
    let layout = state.appearance.layout;
    let y = layout.card_top(surface) + state.content_height() - geom::PAD - geom::FOOTER_H;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_separates_no_results_from_an_untouched_field() {
        // an empty field is the history view, not a failed search
        assert_eq!(footer_hint(0, true, false), "Type ? for help");
        assert_eq!(footer_hint(0, false, false), "No results");
        assert!(footer_hint(3, false, false).starts_with("↵ Launch"));
        // the panel owns the footer while it is open
        assert!(footer_hint(3, false, true).starts_with("↵ Run"));
    }
}
