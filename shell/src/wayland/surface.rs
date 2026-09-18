use std::sync::atomic::Ordering;
use std::time::Instant;

use smithay_client_toolkit::compositor::{CompositorHandler, FrameCallbackData, Region};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use tiny_skia::Pixmap;
use wayland_client::protocol::{wl_output, wl_shm, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::session::{backend, ipc};
use crate::ui::{geom, render};

use super::{Shell, scale};

const NAMESPACE: &str = "QsFlow";

impl Shell {
    // ---- surface lifecycle -------------------------------------------------

    pub fn open(&mut self, now: Instant) {
        if self.layer.is_some() {
            return;
        }

        let surface = self.compositor_state.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some(NAMESPACE),
            None,
        );
        layer.set_anchor(Anchor::all());
        // `-1` is the QML's `ExclusionMode.Ignore`: the surface ignores other
        // surfaces' exclusive zones and the configure is the full output.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        // A size of 0 with every anchor lets the compositor decide: the output.
        layer.set_size(0, 0);

        self.effect = self
            .effect_state
            .get_background_effect(layer.wl_surface(), &self.qh)
            .ok();
        // Both objects must exist before the surface's first commit: the
        // fractional scale arrives as an event on the first one, and the
        // viewport is what maps the scaled buffer back onto the logical size.
        self.fractional = self.fractional_manager.as_ref().map(|manager| {
            manager.get_fractional_scale(layer.wl_surface(), &self.qh, scale::ScaleData)
        });
        self.viewport = self.viewporter.as_ref().map(|viewporter| {
            viewporter.get_viewport(layer.wl_surface(), &self.qh, scale::ViewportData)
        });
        self.viewport_destination = None;
        self.layer = Some(layer);
        // A configure queued for the previous surface must not count as this
        // one's: present/blit would attach a buffer before the first configure.
        self.configured = false;
        self.blur_sent = None;
        self.ime_cursor_sent = None;
        self.first_frame_logged = false;
        // A paste worker from the previous show must not land on this one.
        self.paste_generation = self.paste_generation.wrapping_add(1);

        self.app.shown(now);
        ipc::VISIBLE.store(true, Ordering::Relaxed);
        // The QML's init timer: an empty query means the usage-ranked history,
        // so a show never displays the previous query's payload.
        backend::send("");
        self.open_at = Some(now);
        self.last_present = None;
        // Allocate the frame buffers here rather than on the first present: in
        // resident mode the surface is 1920x1080 by default, so the 8.3MB
        // pixmap and the shared-memory pool would otherwise land on the frame
        // that starts the entrance animation.
        let (physical_w, physical_h) = self.physical_size();
        self.ensure_buffers(physical_w, physical_h);
        if self.timing {
            eprintln!("qsflow: open at +{:?}", self.started.elapsed());
        }

        // A layer surface must be committed once with no buffer attached before
        // the compositor configures it.
        if let Some(layer) = self.layer.as_ref() {
            layer.commit();
        }
        self.enable_ime();
        self.arm_timer();
    }

    pub(super) fn dismiss(&mut self, now: Instant) {
        if self.layer.is_none() {
            return;
        }

        self.disable_ime();
        // The staged text-input state belongs to the surface that is going
        // away: a `done` queued for the old composition must not land on the
        // next show's empty query.
        self.pending.clear();
        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }
        if let Some(fractional) = self.fractional.take() {
            fractional.destroy();
        }
        self.viewport_destination = None;
        // `app.fractional` is retained: it is an output property and lets the
        // next `open` allocate at the exact ratio instead of re-deriving the
        // integer ceiling (a 1.25× output would briefly allocate 3072x1728).
        // The new surface's `PreferredScale` refreshes it on the first commit.
        // Dropping the layer surface destroys it: there is no hide/unmap verb.
        self.layer = None;
        self.configured = false;
        // `destroy`, not a plain drop: wayland-rs only removes an object from
        // the connection when its destructor request is sent, so dropping the
        // proxy would leak one per toggle.
        if let Some(effect) = self.effect.take() {
            effect.destroy();
        }
        self.pool = None;
        self.pixmap = None;
        self.caret_patch = None;
        // Free the decoded icons and the resolution caches: the resident shell
        // sits hidden between shows and these grow with every distinct payload.
        self.icons.clear();
        self.app.icon_cache.clear();
        self.requested_icons.clear();
        self.text.clear_cache();
        self.app.hidden();
        ipc::VISIBLE.store(false, Ordering::Relaxed);

        if self.resident {
            return;
        }

        let _ = now;
        self.exit = true;
    }

    // ---- present -----------------------------------------------------------

    /// The buffer size in pixels: the logical size times the effective scale,
    /// which is the fractional ratio when the compositor offered one.
    fn physical_size(&self) -> (u32, u32) {
        let scale = self.app.scale_factor();
        let (width, height) = self.app.surface;
        (
            ((width as f32) * scale).round() as u32,
            ((height as f32) * scale).round() as u32,
        )
    }

    /// The pixmaps and the shared-memory pool for a physical size.
    fn ensure_buffers(&mut self, physical_w: u32, physical_h: u32) -> bool {
        if physical_w == 0 || physical_h == 0 {
            return false;
        }

        let sized = |pixmap: &Option<Pixmap>| {
            pixmap
                .as_ref()
                .is_some_and(|pixmap| (pixmap.width(), pixmap.height()) == (physical_w, physical_h))
        };
        if !sized(&self.pixmap) {
            self.pixmap = Pixmap::new(physical_w, physical_h);
            self.caret_patch = None;
            // Two full-output frames: one that the compositor is showing and one
            // being assembled. The pool doubles on demand, so this is the bound
            // of a frame-callback-paced present, not a hard cap.
            self.pool =
                SlotPool::new(physical_w as usize * physical_h as usize * 4 * 2, &self.shm).ok();
        }

        self.pixmap.is_some() && self.pool.is_some()
    }

    pub(super) fn present(&mut self, now: Instant) {
        if self.layer.is_none() || !self.configured {
            return;
        }

        if !self.format_chosen {
            self.format_chosen = true;
            let formats = self.shm.formats().to_vec();
            // `ABGR8888` is byte-for-byte the premultiplied RGBA that tiny-skia
            // produces; the mandatory `ARGB8888` needs a per-pixel R/B swap.
            self.shm_format = if formats.contains(&wl_shm::Format::Abgr8888) {
                wl_shm::Format::Abgr8888
            } else {
                wl_shm::Format::Argb8888
            };
            eprintln!(
                "qsflow: wl_shm formats {formats:?} -> {:?}",
                self.shm_format
            );
        }

        let (physical_w, physical_h) = self.physical_size();
        if physical_w == 0 || physical_h == 0 {
            return;
        }

        if !self.ensure_buffers(physical_w, physical_h) {
            return;
        }
        // The fade begins on the first frame that is actually drawn.
        self.app.start_entrance(now);
        {
            let Some(pixmap) = self.pixmap.as_mut() else {
                return;
            };
            render::draw(pixmap, &self.app, &mut self.text, &mut self.icons, now);
        }
        // Capture the caret-free pixels before drawing the caret, so a blink
        // can restore them without a second frame pixmap.
        self.capture_caret_patch();
        if let Some(pixmap) = self.pixmap.as_mut() {
            render::draw_caret(pixmap, &self.app, &mut self.text, now);
        }

        // The blur region is double-buffered state: it must be set before the
        // commit that should carry it, or the first frame goes unblurred.
        self.sync_blur();

        self.blit(now);
    }

    /// Copy the drawn frame into a shared-memory buffer and commit it.
    fn blit(&mut self, now: Instant) {
        if !self.configured {
            return;
        }

        let (physical_w, physical_h) = self.physical_size();
        let (width, height) = self.app.surface;
        let surface = {
            let Some(layer) = self.layer.as_ref() else {
                return;
            };
            layer.wl_surface().clone()
        };

        let format = self.shm_format;
        let stride = (physical_w * 4) as i32;
        let Some(pixmap) = self.pixmap.as_ref() else {
            return;
        };
        let Some(pool) = self.pool.as_mut() else {
            return;
        };
        let Ok((buffer, canvas)) =
            pool.create_buffer(physical_w as i32, physical_h as i32, stride, format)
        else {
            return;
        };
        copy_into(pixmap.data(), canvas, format);

        // With a fractional ratio the buffer is `logical × ratio` and the
        // viewport maps it back; the surface's own scale stays 1. Without one,
        // the integer scale is what says how big a logical pixel is.
        if self.app.uses_viewport() {
            surface.set_buffer_scale(1);
            if self.viewport_destination != Some((width, height)) {
                if let Some(viewport) = self.viewport.as_ref() {
                    viewport.set_destination(width as i32, height as i32);
                }
                self.viewport_destination = Some((width, height));
            }
        } else {
            surface.set_buffer_scale(self.app.scale.max(1));
        }
        surface.damage_buffer(0, 0, physical_w as i32, physical_h as i32);

        // The frame callback must be registered *before* the commit that it
        // should pace: niri takes the pending callback while processing the
        // commit, so a request sent after it only fires on the next commit —
        // which, on an idle launcher, is the next caret blink 500ms later, and
        // the entrance fade then never plays.
        if self.app.animating(now) {
            let frame_surface = surface.clone();
            surface.frame(&self.qh, FrameCallbackData(frame_surface));
        }

        if buffer.attach_to(&surface).is_err() {
            return;
        }
        let Some(layer) = self.layer.as_ref() else {
            return;
        };
        layer.commit();

        if self.timing {
            let now = Instant::now();
            match self.last_present.take() {
                Some(previous) => eprintln!(
                    "qsflow: frame +{:?} (draw+blit)",
                    now.duration_since(previous)
                ),
                None => eprintln!("qsflow: frame (first)"),
            }
            self.last_present = Some(now);
        }

        // `QSFLOW_SNAPSHOT=<path>` writes the buffer as drawn, once per show:
        // the only way to tell what the shell produced from what the compositor
        // did with it.
        if let Some(path) = std::env::var_os("QSFLOW_SNAPSHOT")
            && !self.first_frame_logged
        {
            let pixmap = self.pixmap.as_ref();
            if let Some(pixmap) = pixmap {
                let _ = pixmap.save_png(&path);
                eprintln!(
                    "qsflow: snapshot {}x{} at scale {} -> {}",
                    pixmap.width(),
                    pixmap.height(),
                    self.app.scale_factor(),
                    path.to_string_lossy()
                );
            }
        }

        if !self.first_frame_logged {
            self.first_frame_logged = true;
            if self.timing
                && let Some(at) = self.open_at
            {
                eprintln!(
                    "qsflow: first frame {}us ({}x{} at scale {})",
                    at.elapsed().as_micros(),
                    physical_w,
                    physical_h,
                    self.app.scale_factor()
                );
            }
        }

        self.sync_ime_cursor();
    }

    /// A blink: repaint the caret over the restored patch and commit, without
    /// re-rendering the surface.
    pub(super) fn present_caret(&mut self, now: Instant) {
        self.restore_caret_patch();
        if let Some(pixmap) = self.pixmap.as_mut() {
            render::draw_caret(pixmap, &self.app, &mut self.text, now);
        }
        self.blit(Instant::now());
    }

    /// Save the pixels the caret covers: the frame without it, in a rect just
    /// around the caret. A blink restores this patch and redraws the caret, so
    /// the shell never holds a second full-size frame.
    fn capture_caret_patch(&mut self) {
        let (x, y, w, h) = render::caret_rect(&self.app, &mut self.text);
        let scale = self.app.scale_factor();
        let Some(pixmap) = self.pixmap.as_ref() else {
            self.caret_patch = None;
            return;
        };

        // The caret is drawn with AA at fractional scales, so widen the patch by
        // two physical pixels on each side and clamp it into the pixmap.
        let left = (x as f32 * scale - 2.0).max(0.0).floor() as u32;
        let top = (y as f32 * scale - 2.0).max(0.0).floor() as u32;
        let right = ((x + w) as f32 * scale + 2.0)
            .min(pixmap.width() as f32)
            .ceil() as u32;
        let bottom = ((y + h) as f32 * scale + 2.0)
            .min(pixmap.height() as f32)
            .ceil() as u32;
        self.caret_patch = CaretPatch::capture(
            pixmap,
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        );
    }

    /// Put the caret-free pixels back, so the caret can be drawn (or not) again.
    fn restore_caret_patch(&mut self) {
        let (Some(pixmap), Some(patch)) = (self.pixmap.as_mut(), self.caret_patch.as_ref()) else {
            return;
        };
        patch.restore(pixmap);
    }

    /// The blur region covers the card's rounded rect. The runtime re-applies
    /// the stored region on every present, so the send is deduped on the
    /// 4px-quantised rect.
    fn sync_blur(&mut self) {
        let (Some(layer), Some(effect)) = (self.layer.as_ref(), self.effect.as_ref()) else {
            return;
        };
        let _ = layer;

        let height = self.app.card_height(Instant::now());
        let rects = geom::blur_rects(self.app.surface, height);
        let key = rects
            .first()
            .map(|rect| (rect.x, rect.y, rect.width, rect.height));
        if key.is_none() || self.blur_sent == key {
            return;
        }

        let Ok(region) = Region::new(&self.compositor_state) else {
            return;
        };
        for rect in &rects {
            region.add(rect.x, rect.y, rect.width, rect.height);
        }
        effect.set_blur_region(Some(region.wl_region()));
        self.blur_sent = key;
    }
}

/// The pixels under the caret, saved from a frame drawn without it. A blink
/// restores the patch and redraws the caret, so the shell keeps a few hundred
/// bytes instead of a second full-output pixmap.
#[derive(Debug)]
pub(super) struct CaretPatch {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    pixels: Vec<u8>,
}

impl CaretPatch {
    /// `w`/`h` of zero, or any overflow of the pixmap, gives no patch.
    fn capture(pixmap: &Pixmap, x: u32, y: u32, w: u32, h: u32) -> Option<Self> {
        if w == 0 || h == 0 {
            return None;
        }
        if x as usize + w as usize > pixmap.width() as usize
            || y as usize + h as usize > pixmap.height() as usize
        {
            return None;
        }

        let stride = pixmap.width() as usize * 4;
        let row = w as usize * 4;
        let mut pixels = Vec::with_capacity(row * h as usize);
        for line in 0..h {
            let start = (y + line) as usize * stride + x as usize * 4;
            pixels.extend_from_slice(&pixmap.data()[start..start + row]);
        }
        Some(Self { x, y, w, h, pixels })
    }

    fn restore(&self, pixmap: &mut Pixmap) {
        let stride = pixmap.width() as usize * 4;
        let row = self.w as usize * 4;
        let data = pixmap.data_mut();
        for line in 0..self.h {
            let start = (self.y + line) as usize * stride + self.x as usize * 4;
            let source = line as usize * row;
            data[start..start + row].copy_from_slice(&self.pixels[source..source + row]);
        }
    }
}

/// The pixmap is premultiplied RGBA; `ABGR8888` is that byte for byte, while
/// `ARGB8888` is the same word with R and B swapped.
fn copy_into(source: &[u8], destination: &mut [u8], format: wl_shm::Format) {
    // `sctk` rounds a slot's length up to 64 bytes, so the canvas can be longer
    // than the pixmap; copying the whole slice would panic.
    let Some(canvas) = destination.get_mut(..source.len()) else {
        return;
    };

    if format == wl_shm::Format::Abgr8888 {
        canvas.copy_from_slice(source);
        return;
    }

    for (target, pixel) in canvas.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
        target[0] = pixel[2];
        target[1] = pixel[1];
        target[2] = pixel[0];
        target[3] = pixel[3];
    }
}

impl CompositorHandler for Shell {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if self
            .layer
            .as_ref()
            .is_none_or(|layer| layer.wl_surface() != surface)
        {
            return;
        }

        self.app.scale = factor.max(1);
        self.redraw();
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.present(Instant::now());
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for Shell {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.dismiss(Instant::now());
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // A configure for a surface that is gone (or a superseded one) must not
        // flip `configured` and let the next show present before its own.
        if self
            .layer
            .as_ref()
            .is_none_or(|current| current.wl_surface() != layer.wl_surface())
        {
            return;
        }

        let (width, height) = configure.new_size;
        if width == 0 || height == 0 {
            return;
        }

        if self.timing {
            eprintln!(
                "qsflow: configure {width}x{height} at +{:?}",
                self.started.elapsed()
            );
        }
        self.app.surface = (width, height);
        self.configured = true;
        self.blur_sent = None;

        self.present(Instant::now());
        let _ = self.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::{CaretPatch, copy_into};
    use tiny_skia::Pixmap;
    use wayland_client::protocol::wl_shm;

    #[test]
    fn a_longer_shm_canvas_is_not_a_panic() {
        // `sctk` rounds a slot up to 64 bytes, so the canvas is longer than the
        // pixmap whenever the buffer's byte length is not a multiple of 64 —
        // which is any output whose pixel count is not a multiple of 16.
        let source: Vec<u8> = (0..16).collect();
        let mut destination = vec![0u8; 16 + 48];

        copy_into(&source, &mut destination, wl_shm::Format::Abgr8888);
        assert_eq!(&destination[..16], &source[..]);
        assert!(destination[16..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn argb8888_swaps_red_and_blue() {
        let source = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut destination = vec![0u8; 8];

        copy_into(&source, &mut destination, wl_shm::Format::Argb8888);
        assert_eq!(destination, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn a_short_canvas_writes_nothing_rather_than_panicking() {
        let source = vec![1u8; 16];
        let mut destination = vec![0u8; 4];

        copy_into(&source, &mut destination, wl_shm::Format::Abgr8888);
        assert_eq!(destination, vec![0u8; 4]);
    }

    #[test]
    fn a_caret_patch_restores_only_its_own_rect() {
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        for (i, byte) in pixmap.data_mut().iter_mut().enumerate() {
            *byte = i as u8;
        }
        let before = pixmap.data().to_vec();

        let patch = CaretPatch::capture(&pixmap, 1, 1, 2, 1).unwrap();
        // Paint over the patch and elsewhere: only the patch's rect comes back.
        for byte in &mut pixmap.data_mut()[0..8] {
            *byte = 255;
        }
        for byte in &mut pixmap.data_mut()[32..44] {
            *byte = 255;
        }
        patch.restore(&mut pixmap);

        let stride = 4 * 4;
        let row = 1;
        assert_eq!(
            &pixmap.data()[row * stride + 4..row * stride + 12],
            &before[row * stride + 4..row * stride + 12]
        );
        assert_eq!(&pixmap.data()[0..8], &[255u8; 8]);
        assert_eq!(&pixmap.data()[32..44], &[255u8; 12]);
    }

    #[test]
    fn a_caret_patch_outside_the_pixmap_is_dropped() {
        let pixmap = Pixmap::new(4, 4).unwrap();
        assert!(CaretPatch::capture(&pixmap, 3, 0, 2, 1).is_none());
        assert!(CaretPatch::capture(&pixmap, 0, 3, 1, 2).is_none());
        assert!(CaretPatch::capture(&pixmap, 0, 0, 0, 1).is_none());
    }
}
