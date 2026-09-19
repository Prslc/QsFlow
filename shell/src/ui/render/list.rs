use std::time::Instant;

use cosmic_text::Weight;
use tiny_skia::Pixmap;

use crate::app::{Hover, State};
use crate::ui::geom;
use crate::ui::icons::IconCache;
use crate::ui::text::TextEngine;

use super::canvas::{BADGE_SIZE, Canvas, ICON_SIZE, Rect, SUMMARY_SIZE, TITLE_SIZE};

pub(super) fn draw_list(
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
        // The selected row's ↵ hint and the pinned badge are part of the
        // layout: the labels must leave room for both, so a long title cannot
        // run underneath either.
        let enter = selected.then(|| text.shape("↵", 13.0 * canvas.scale, Weight::NORMAL));
        let enter_w = enter
            .as_ref()
            .map_or(0.0, |shaped| shaped.width / canvas.scale + 12.0);
        let badge_w = row.badge.as_deref().map_or(0.0, |_| BADGE_SIZE + 8.0);
        let labels_max = (rect.right() - 10.0 - labels_x - enter_w - badge_w).max(0.0);

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

        if let Some(path) = row.badge.as_deref() {
            let x = rect.right() - 10.0 - enter_w - BADGE_SIZE;
            icons.draw_tinted(
                pixmap,
                path,
                (canvas.px(x), canvas.px(rect.center_y() - BADGE_SIZE / 2.0)),
                (BADGE_SIZE * canvas.scale).round() as u32,
                state.entrance(now),
                theme.primary,
            );
        }
    }
}
