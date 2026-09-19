pub const ROW_H: f32 = 64.0;
pub const MAX_ROWS: usize = 5;
pub const PAD: f32 = 14.0;
pub const SEARCH_H: f32 = 52.0;
pub const GAP: f32 = 10.0;
pub const FOOTER_H: f32 = 28.0;
pub const RADIUS: f32 = 16.0;
pub const CARD_MAX_H: f32 = 480.0;

/// Backdrop dim at full opacity.
pub const DIM_ALPHA: f32 = 0.30;
/// Card fill alpha — translucent so the compositor blur reads as frost.
pub const CARD_ALPHA: f32 = 0.72;

/// One rect of the blur region, in surface-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlurRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn card_w(surface: (u32, u32)) -> f32 {
    ((surface.0 as f32) * 0.38).clamp(560.0, 760.0)
}

pub fn card_x(surface: (u32, u32)) -> f32 {
    (((surface.0 as f32) - card_w(surface)) / 2.0).round()
}

/// Fixed card top: results only extend the card downward, so the search bar
/// never moves when the row count changes.
pub fn card_top(surface: (u32, u32)) -> f32 {
    ((surface.1 as f32) * 0.28).round()
}

/// The y of the first list row: the card's padding, the search field and the
/// column's spacing.
pub fn rows_top(surface: (u32, u32)) -> f32 {
    card_top(surface) + PAD + SEARCH_H + GAP
}

pub fn list_h(rows: usize) -> f32 {
    (rows as f32 * ROW_H).min(MAX_ROWS as f32 * ROW_H)
}

/// `pad + search + gap + footer + pad`, plus a second gap and the list when
/// there are rows (with no rows the column holds one gap, not two).
pub fn content_h(rows: usize) -> f32 {
    let base = PAD + SEARCH_H + GAP + FOOTER_H + PAD;
    let height = if rows == 0 {
        base
    } else {
        base + GAP + list_h(rows)
    };
    height.min(CARD_MAX_H)
}

/// The blur region covering the card's rounded rect. A rect cannot carry a
/// radius, so the corners are 2px scanline bands inset by
/// `r - sqrt(r² - (r - dy)²)`, taken at each band's top so no rect pokes
/// outside the rounded edge.
pub fn blur_rects(surface: (u32, u32), card_height: f32) -> Vec<BlurRect> {
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
        rects.push(BlurRect {
            x,
            y,
            width,
            height,
        });
        return rects;
    }

    rects.push(BlurRect {
        x,
        y: y + radius,
        width,
        height: height - 2 * radius,
    });

    let r = radius as f32;
    for offset in (0..radius).step_by(2) {
        let f = offset as f32;
        let inset = (r - (r * r - (r - f) * (r - f)).max(0.0).sqrt()).floor() as i32;
        let band = BlurRect {
            x: x + inset,
            y: y + offset,
            width: width - 2 * inset,
            height: 2,
        };
        rects.push(band);
        rects.push(BlurRect {
            y: y + height - offset - 2,
            ..band
        });
    }

    rects
}

/// The row index under a surface-local point, `None` outside the list band.
pub fn row_at(surface: (u32, u32), first: usize, rows: usize, x: f32, y: f32) -> Option<usize> {
    let left = card_x(surface);
    if x < left || x > left + card_w(surface) {
        return None;
    }

    let top = rows_top(surface);
    if y < top || y >= top + list_h(rows) {
        return None;
    }

    let index = first + ((y - top) / ROW_H) as usize;
    (index < rows).then_some(index)
}

/// `Contain` semantics for the fixed [`MAX_ROWS`]-row window.
pub fn contain(selected: usize, first: usize, rows: usize) -> usize {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_at_inverts_the_layout_the_renderer_draws() {
        let surface = (1920, 1080);
        let left = card_x(surface) + 10.0;
        let top = rows_top(surface);

        assert_eq!(row_at(surface, 0, 20, left, top), Some(0));
        assert_eq!(row_at(surface, 0, 20, left, top + ROW_H + 1.0), Some(1));
        // `first` is the window's top row, so surface row 0 is `first`
        assert_eq!(row_at(surface, 2, 20, left, top + 0.5), Some(2));

        // only the five-row window is drawn
        assert_eq!(
            row_at(surface, 0, 20, left, top + MAX_ROWS as f32 * ROW_H),
            None
        );
        // outside the card, and above the list
        assert_eq!(row_at(surface, 0, 20, card_x(surface) - 1.0, top), None);
        assert_eq!(row_at(surface, 0, 20, left, top - 1.0), None);
        // a short list has nothing below its last row
        assert_eq!(row_at(surface, 0, 2, left, top + 2.0 * ROW_H + 1.0), None);
    }

    #[test]
    fn the_blur_region_covers_the_card_and_stays_inside_it() {
        let surface = (1920, 1080);
        let height = content_h(MAX_ROWS);
        let (x, y) = (
            card_x(surface).round() as i32,
            card_top(surface).round() as i32,
        );
        let (w, h) = (card_w(surface).round() as i32, height.round() as i32);
        let rects = blur_rects(surface, height);

        // the region is always the card's outer rect plus its scanline bands
        for rect in &rects {
            assert!(rect.width > 0 && rect.height > 0, "{rect:?}");
            assert!(rect.x >= x && rect.x + rect.width <= x + w, "{rect:?}");
            assert!(rect.y >= y && rect.y + rect.height <= y + h, "{rect:?}");
        }
        // the first band is the middle one: inset by the radius top and bottom
        assert_eq!(rects[0].y, y + RADIUS as i32);
        assert_eq!(rects[0].height, h - 2 * RADIUS as i32);
        // the corner bands are pulled in by the chord inset, most at the very
        // top of the card, and they alternate top/bottom per offset
        assert_eq!(rects[1].x, x + RADIUS as i32);
        assert_eq!(rects[1].y, y);
        assert_eq!(rects[2].y + 2, y + h);
        // a card shorter than two radii degrades to one rect
        assert_eq!(blur_rects(surface, 20.0).len(), 1);
    }
}
