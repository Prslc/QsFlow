//! Card geometry and the frosted-glass blur region, ported from
//! `ui/SearchWindow.qml`.

use iced::Size;
use iced_exwlshell::reexport::BlurRegion;

pub const ROW_H: f32 = 64.0;
pub const MAX_ROWS: usize = 5;
pub const PAD: f32 = 14.0;
pub const SEARCH_H: f32 = 52.0;
pub const GAP: f32 = 10.0;
pub const FOOTER_H: f32 = 28.0;
pub const RADIUS: f32 = 16.0;
pub const CARD_MAX_H: f32 = 480.0;

/// Backdrop dim at full opacity (the QML's `dim.opacity = 0.30`).
pub const DIM_ALPHA: f32 = 0.30;
/// Card fill alpha — translucent so the compositor blur reads as frost.
pub const CARD_ALPHA: f32 = 0.72;

pub const ENTRANCE_MS: u64 = 240;
pub const REFLOW_MS: u64 = 150;
/// `exitTimer` in the QML: launch dismissals wait this long so the click has
/// time to land before the surface goes away.
pub const EXIT_DELAY_MS: u64 = 150;

pub fn card_w(surface: Size) -> f32 {
    (surface.width * 0.38).clamp(560.0, 760.0)
}

pub fn card_x(surface: Size) -> f32 {
    ((surface.width - card_w(surface)) / 2.0).round()
}

/// Fixed card top in the upper half of the screen: results only extend the
/// card downward, so the search bar never moves when the row count changes.
pub fn card_top(surface: Size) -> f32 {
    (surface.height * 0.28).round()
}

/// The y of the first list row: the card's padding, the search field and the
/// column's spacing.
pub fn rows_top(surface: Size) -> f32 {
    card_top(surface) + PAD + SEARCH_H + GAP
}

pub fn list_h(rows: usize) -> f32 {
    (rows as f32 * ROW_H).min(MAX_ROWS as f32 * ROW_H)
}

/// The card's real height: `pad + search + gap + footer + pad`, plus a second
/// gap and the list when there are rows. There is no separator element: with no
/// rows the column holds `search + footer` and exactly one gap.
pub fn content_h(rows: usize) -> f32 {
    let base = PAD + SEARCH_H + GAP + FOOTER_H + PAD;
    let height = if rows == 0 {
        base
    } else {
        base + GAP + list_h(rows)
    };
    height.min(CARD_MAX_H)
}

/// The blur region covering the card's rounded rect, in surface-local
/// coordinates. A single rect cannot carry a radius, so the corners are
/// approximated by 2px scanline bands whose inset follows the circle:
/// `inset(dy) = r - sqrt(r² - (r - dy)²)`. The band inset is taken at the top
/// of each band, which keeps every rect inside the card (never a blurred sliver
/// outside the rounded edge).
pub fn blur_rects(surface: Size, card_height: f32) -> Vec<BlurRegion> {
    // Floor to 4px so an in-flight reflow does not commit a region per frame.
    let height = ((card_height / 4.0).floor() * 4.0).max(4.0);
    let (x, y) = (
        card_x(surface).round() as i32,
        card_top(surface).round() as i32,
    );
    let width = card_w(surface).round() as i32;
    let height = height.round() as i32;
    let radius = RADIUS.round() as i32;

    let mut rects = Vec::with_capacity(radius as usize + 4);

    if height <= 2 * radius {
        rects.push(BlurRegion {
            x,
            y,
            width,
            height,
        });
        return rects;
    }

    // middle band: full width, between the corner radii
    rects.push(BlurRegion {
        x,
        y: y + radius,
        width,
        height: height - 2 * radius,
    });

    let r = radius as f32;
    for offset in (0..radius).step_by(2) {
        let f = offset as f32;
        let inset = (r - (r * r - (r - f) * (r - f)).max(0.0).sqrt()).floor() as i32;
        let band = BlurRegion {
            x: x + inset,
            y: y + offset,
            width: width - 2 * inset,
            height: 2,
        };
        rects.push(band);
        rects.push(BlurRegion {
            y: y + height - offset - 2,
            ..band
        });
    }

    rects
}
