use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};
use tiny_skia::Pixmap;

/// The machine's CJK-capable sans default; cosmic-text falls back per glyph for
/// anything it lacks.
pub const FAMILY: &str = "Source Han Sans CN";

/// The card's line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.2;

pub struct Shaped {
    buffer: Buffer,
    /// The line's advance width, for the caret and right-aligned text.
    pub width: f32,
    /// Line-box height (`size * LINE_HEIGHT`).
    pub height: f32,
}

/// The shaped-line cache key: the text plus its size (as bits — `f32` is not
/// `Eq`) and weight. A card redraw reshapes the same row titles on every
/// animation frame, so a hit is the common case.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeKey {
    text: String,
    size: u32,
    weight: Weight,
}

/// Distinct lines kept before the cache is dropped wholesale. A show's live set
/// is tiny (five rows, the query, the footer); the cap only guards a long typing
/// session, and `clear_cache` frees everything on dismiss anyway.
const SHAPE_CACHE_MAX: usize = 256;

pub struct TextEngine {
    font_system: FontSystem,
    cache: SwashCache,
    shapes: HashMap<ShapeKey, Rc<Shaped>>,
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            cache: SwashCache::new(),
            shapes: HashMap::new(),
        }
    }

    /// Drop the rasterised-glyph bitmaps and the shaped-line cache. The
    /// `FontSystem`'s font database is process-lifetime, but the glyph cache
    /// grows with every distinct glyph and size a show draws, so the resident
    /// shell frees it on dismiss.
    pub fn clear_cache(&mut self) {
        self.cache = SwashCache::new();
        self.shapes.clear();
        release_font_pages(&self.font_system);
    }

    /// Shape one unwrapped line at `size`, returning a handle to the cached
    /// layout. The same line, size and weight is shaped once; every later frame
    /// that draws it is a hash lookup.
    pub fn shape(&mut self, text: &str, size: f32, weight: Weight) -> Rc<Shaped> {
        let key = ShapeKey {
            text: text.to_string(),
            size: size.to_bits(),
            weight,
        };
        if let Some(shaped) = self.shapes.get(&key) {
            return Rc::clone(shaped);
        }

        let shaped = Rc::new(self.shape_uncached(text, size, weight));
        if self.shapes.len() >= SHAPE_CACHE_MAX {
            self.shapes.clear();
        }
        self.shapes.insert(key, Rc::clone(&shaped));
        shaped
    }

    fn shape_uncached(&mut self, text: &str, size: f32, weight: Weight) -> Shaped {
        let line_height = (size * LINE_HEIGHT).round();
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_height));
        // No constraints: a single line, whatever its width (long titles are
        // clipped by the caller).
        buffer.set_size(&mut self.font_system, None, None);

        let attrs = Attrs::new().family(Family::Name(FAMILY)).weight(weight);
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max);

        Shaped {
            buffer,
            width,
            height: line_height,
        }
    }

    /// Shape one line, eliding it with `…` so it fits `max_width`. A line that
    /// already fits is shaped as-is, which is the common case and costs one
    /// shape; only an overlong line pays the truncation search.
    pub fn fit(&mut self, text: &str, size: f32, weight: Weight, max_width: f32) -> Rc<Shaped> {
        let full = self.shape(text, size, weight);
        if full.width <= max_width {
            return full;
        }

        let ellipsis = self.shape("…", size, weight);
        if max_width <= ellipsis.width {
            return ellipsis;
        }

        // The largest char-boundary prefix that leaves room for the ellipsis,
        // found by binary search over the byte boundaries so a long string
        // costs a handful of shapes rather than one per character. The prefix
        // shapes are cached, so a later frame only pays the search.
        let mut boundaries: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
        boundaries.push(text.len());
        let (mut low, mut high) = (0, boundaries.len() - 1);
        while low < high {
            let mid = low + (high - low).div_ceil(2);
            let width = self.shape(&text[..boundaries[mid]], size, weight).width;
            if width + ellipsis.width <= max_width {
                low = mid;
            } else {
                high = mid - 1;
            }
        }

        let mut display = String::with_capacity(boundaries[low] + 4);
        display.push_str(&text[..boundaries[low]]);
        display.push('…');
        self.shape(&display, size, weight)
    }

    /// Draw a shaped line with its line-box's top-left corner at `(x, top)`.
    /// Pixels outside `clip` (`[x, y, w, h]`) are dropped.
    pub fn draw(
        &mut self,
        pixmap: &mut Pixmap,
        shaped: &Shaped,
        color: [u8; 4],
        x: f32,
        top: f32,
        clip: Option<[f32; 4]>,
    ) {
        let width = pixmap.width() as i32;
        let height = pixmap.height() as i32;
        let data = pixmap.data_mut();
        let Self {
            font_system, cache, ..
        } = self;

        for run in shaped.buffer.layout_runs() {
            // `glyph.y` is the offset within the line box, not the baseline:
            // without the run's baseline the text sits a line-ascent too high.
            for glyph in run.glyphs {
                let physical = glyph.physical((x, top + run.line_y), 1.0);
                let Some(image) = cache.get_image(font_system, physical.cache_key) else {
                    continue;
                };

                let left = physical.x + image.placement.left;
                let top = physical.y - image.placement.top;
                let (iw, ih) = (image.placement.width, image.placement.height);

                let mut i = 0;
                for oy in 0..ih as i32 {
                    for ox in 0..iw as i32 {
                        let (px, py) = (left + ox, top + oy);
                        let inside = clip.is_none_or(|[cx, cy, cw, ch]| {
                            (px as f32) >= cx
                                && (py as f32) >= cy
                                && (px as f32) < cx + cw
                                && (py as f32) < cy + ch
                        });

                        match image.content {
                            SwashContent::Mask => {
                                let alpha = image.data[i];
                                i += 1;
                                if inside {
                                    let alpha =
                                        (u16::from(alpha) * u16::from(color[3]) / 255) as u8;
                                    blend(data, width, height, px, py, color, alpha);
                                }
                            }
                            SwashContent::Color => {
                                // A bitmap glyph (an emoji) is straight RGBA:
                                // its own alpha is the coverage.
                                let alpha = (u16::from(image.data[i + 3]) * u16::from(color[3])
                                    / 255) as u8;
                                if inside {
                                    blend(
                                        data,
                                        width,
                                        height,
                                        px,
                                        py,
                                        [
                                            image.data[i],
                                            image.data[i + 1],
                                            image.data[i + 2],
                                            image.data[i + 3],
                                        ],
                                        alpha,
                                    );
                                }
                                i += 4;
                            }
                            SwashContent::SubpixelMask => {
                                i += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Source-over one straight-alpha pixel into a premultiplied RGBA buffer.
fn blend(data: &mut [u8], width: i32, height: i32, x: i32, y: i32, color: [u8; 4], alpha: u8) {
    if alpha == 0 || x < 0 || y < 0 || x >= width || y >= height {
        return;
    }

    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let index = ((y * width + x) * 4) as usize;
    let premultiplied = |channel: u8| (u32::from(channel) * alpha + 127) / 255;

    for (offset, channel) in color[..3].iter().enumerate() {
        let source = premultiplied(*channel);
        let destination = u32::from(data[index + offset]) * inverse / 255;
        data[index + offset] = (source + destination).min(255) as u8;
    }

    let destination = u32::from(data[index + 3]) * inverse / 255;
    data[index + 3] = (alpha + destination).min(255) as u8;
}

/// Hand the resident pages of every mmap'd font file back to the OS. Shaping a
/// CJK line faults a font's tables in, and neither the glyph cache nor
/// `malloc_trim` touches a file mapping, so a hidden shell would otherwise keep
/// the whole page set. The mapping stays valid: the next shape re-faults.
#[cfg(target_os = "linux")]
fn release_font_pages(font_system: &FontSystem) {
    let mut seen: HashSet<*const u8> = HashSet::new();
    for face in font_system.db().faces() {
        let fontdb::Source::SharedFile(_, data) = &face.source else {
            continue;
        };
        let bytes: &[u8] = <dyn AsRef<[u8]>>::as_ref(&**data);
        if bytes.is_empty() || !seen.insert(bytes.as_ptr()) {
            continue;
        }
        // SAFETY: `bytes` is a live mmap from `fontdb`, so the base is page
        // aligned; `madvise` rounds the length and has no other precondition.
        unsafe {
            libc::madvise(
                bytes.as_ptr() as *mut libc::c_void,
                bytes.len(),
                libc::MADV_DONTNEED,
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn release_font_pages(_: &FontSystem) {}

#[cfg(test)]
mod tests {
    use super::{TextEngine, blend};
    use cosmic_text::Weight;

    #[test]
    fn the_same_line_is_shaped_once() {
        let mut engine = TextEngine::new();
        let first = engine.shape("Firefox", 14.0, Weight::BOLD);
        let second = engine.shape("Firefox", 14.0, Weight::BOLD);
        assert!(std::rc::Rc::ptr_eq(&first, &second), "cache hit");

        // a different size is a different key
        let other = engine.shape("Firefox", 12.0, Weight::BOLD);
        assert!(!std::rc::Rc::ptr_eq(&first, &other));
        // and a cleared cache re-shapes
        engine.clear_cache();
        let after = engine.shape("Firefox", 14.0, Weight::BOLD);
        assert!(!std::rc::Rc::ptr_eq(&first, &after));
    }

    #[test]
    fn a_fitting_line_is_shaped_unchanged() {
        let mut engine = TextEngine::new();
        let full = engine.shape("Short", 14.0, Weight::NORMAL);
        let fitted = engine.fit("Short", 14.0, Weight::NORMAL, full.width + 1.0);
        assert_eq!(fitted.width, full.width);
    }

    #[test]
    fn an_overlong_line_is_elided_to_the_limit() {
        let mut engine = TextEngine::new();
        let long = "a title that is far too long to ever fit on one card row";
        let full = engine.shape(long, 14.0, Weight::BOLD);
        let max = full.width / 3.0;
        let fitted = engine.fit(long, 14.0, Weight::BOLD, max);
        assert!(fitted.width <= max + 1.0, "{} > {max}", fitted.width);
        assert!(fitted.width < full.width);
    }

    #[test]
    fn glyph_blending_is_premultiplied_source_over() {
        // half coverage over opaque white: the colour is premultiplied by the
        // coverage and the destination keeps the rest
        let mut data = [0u8; 8];
        data[0..4].copy_from_slice(&[255, 255, 255, 255]);
        blend(&mut data, 2, 1, 0, 0, [255, 0, 0, 255], 128);
        assert_eq!(&data[0..3], &[255, 127, 127]);
        assert_eq!(data[3], 255);

        // full coverage over a transparent pixel is the colour itself
        let mut data = [0u8; 8];
        blend(&mut data, 2, 1, 1, 0, [255, 0, 0, 255], 255);
        assert_eq!(&data[4..7], &[255, 0, 0]);
        assert_eq!(data[7], 255);

        // zero coverage writes nothing
        let mut data = [0u8; 8];
        blend(&mut data, 2, 1, 0, 0, [255, 0, 0, 255], 0);
        assert_eq!(data, [0u8; 8]);

        // out of bounds is a no-op, not a panic
        let mut data = [0u8; 8];
        blend(&mut data, 2, 1, -1, 9, [255, 0, 0, 255], 255);
        assert_eq!(data, [0u8; 8]);
    }
}
