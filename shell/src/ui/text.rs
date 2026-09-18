use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight,
};
use tiny_skia::Pixmap;

/// The machine's CJK-capable default (`fc-match sans:lang=zh-cn`); cosmic-text
/// falls back per glyph for anything it lacks.
pub const FAMILY: &str = "Source Han Sans CN";

/// The card's line height: 1.2 × the font size, as the QML's text used.
pub const LINE_HEIGHT: f32 = 1.2;

pub struct Shaped {
    buffer: Buffer,
    /// The line's advance width, for the caret and right-aligned text.
    pub width: f32,
    /// Line-box height (`size * LINE_HEIGHT`).
    pub height: f32,
}

pub struct TextEngine {
    font_system: FontSystem,
    cache: SwashCache,
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    /// Shape one unwrapped line at `size`.
    pub fn shape(&mut self, text: &str, size: f32, weight: Weight) -> Shaped {
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
        let Self { font_system, cache } = self;

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
                                // its own alpha is the coverage. Using a fixed
                                // 255 wrote the whole placement box opaque,
                                // transparent padding included.
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

#[cfg(test)]
mod tests {
    use super::blend;

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
