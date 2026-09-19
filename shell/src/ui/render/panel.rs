use std::time::Instant;

use cosmic_text::Weight;
use tiny_skia::Pixmap;

use crate::app::{Hover, State};
use crate::ui::geom;
use crate::ui::icons::IconCache;
use crate::ui::text::TextEngine;

use super::canvas::{Canvas, ICON_SIZE, Rect, SUGGESTION_SIZE, TITLE_SIZE};

/// The action panel (Shift+Enter): the parent row's title as a header, then the
/// row's secondary commands as list rows in the same fixed window.
pub(super) fn draw_actions(
    canvas: &Canvas,
    pixmap: &mut Pixmap,
    surface: (u32, u32),
    state: &State,
    text: &mut TextEngine,
    icons: &mut IconCache,
    now: Instant,
) {
    let Some(menu) = &state.menu else {
        return;
    };
    let theme = state.theme;
    let layout = state.appearance.layout;
    let left = layout.card_x(surface) + geom::PAD;
    let width = layout.card_w(surface) - 2.0 * geom::PAD;

    if let Some(title) = state.menu_parent_title() {
        let size = SUGGESTION_SIZE * canvas.scale;
        let shaped = text.fit(title, size, Weight::BOLD, width * canvas.scale);
        let y = layout.rows_top(surface);
        let clip = [
            canvas.px(left),
            canvas.px(y),
            canvas.px(width),
            canvas.px(layout.panel_header_h()),
        ];
        text.draw(
            pixmap,
            &shaped,
            state.fade(theme.primary, 0.9, now),
            canvas.px(left),
            canvas.px(y + (layout.panel_header_h() - shaped.height / canvas.scale) / 2.0),
            Some(clip),
        );
    }

    let top = layout.actions_top(surface);
    for (index, action) in menu
        .actions
        .iter()
        .enumerate()
        .skip(menu.first)
        .take(layout.max_rows)
    {
        let y = top + (index - menu.first) as f32 * geom::ROW_H;
        let rect = Rect {
            x: left,
            y,
            w: width,
            h: geom::ROW_H,
        };

        let selected = index == menu.selected;
        let hovered = state.hovered == Some(Hover::Action(index));
        let background = match (selected, hovered) {
            (true, _) => Some(state.fade(theme.primary, 0.15, now)),
            (false, true) => Some(state.fade(theme.primary, 0.08, now)),
            (false, false) => None,
        };
        if let Some(background) = background {
            canvas.fill_round(pixmap, rect, layout.row_radius(), background);
        }

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

        // Always 3px wide so the icon sits at the same x on every action; only
        // the selected one paints an accent bar.
        let icon_x = rect.x + 11.0;
        if let Some(path) = action.icon.as_deref() {
            icons.draw_tinted(
                pixmap,
                path,
                (
                    canvas.px(icon_x),
                    canvas.px(rect.center_y() - ICON_SIZE / 2.0),
                ),
                (ICON_SIZE * canvas.scale).round() as u32,
                state.entrance(now),
                theme.fg,
            );
        }

        let labels_x = icon_x + ICON_SIZE + 12.0;
        let enter = selected.then(|| text.shape("↵", 13.0 * canvas.scale, Weight::NORMAL));
        let enter_w = enter
            .as_ref()
            .map_or(0.0, |shaped| shaped.width / canvas.scale + 12.0);
        let labels_max = (rect.right() - 10.0 - labels_x - enter_w).max(0.0);
        let title = text.fit(
            &action.title,
            TITLE_SIZE * canvas.scale,
            Weight::NORMAL,
            labels_max * canvas.scale,
        );
        let title_h = title.height / canvas.scale;
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
            canvas.px(rect.center_y() - title_h / 2.0),
            Some(clip),
        );
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
