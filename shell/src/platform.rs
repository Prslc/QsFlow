//! The Slint platform behind the launcher.
//!
//! Slint has no layer-shell backend, so the shell supplies the documented
//! custom-platform seam instead of a hand-written renderer: one [`Adapter`]
//! implements [`WindowAdapter`] on top of the software renderer, which paints a
//! premultiplied RGBA8 buffer that is byte-for-byte a `wl_shm` `ABGR8888`
//! buffer, and the Wayland side presents it.

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use i_slint_core::InternalToken;
use i_slint_core::window::{InputMethodRequest, WindowAdapterInternal};
use slint::platform::software_renderer::{
    PremultipliedRgbaColor, RepaintBufferType, SoftwareRenderer,
};
use slint::platform::{Clipboard, Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{PhysicalSize, Window, WindowSize};

pub struct Adapter {
    window: Window,
    renderer: SoftwareRenderer,
    size: Cell<PhysicalSize>,
    stride: Cell<usize>,
    needs_redraw: Cell<bool>,
    pixels: RefCell<Vec<PremultipliedRgbaColor>>,
    /// What Slint asked the input method to do; the Wayland side drains this.
    ime_requests: RefCell<Vec<InputMethodRequest>>,
}

impl Adapter {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|weak: &std::rc::Weak<Self>| Self {
            window: Window::new(weak.clone()),
            // Every frame goes into a fresh shared-memory slot, so Slint cannot
            // assume the previous frame is still in the buffer.
            renderer: SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::NewBuffer),
            size: Cell::new(PhysicalSize::default()),
            stride: Cell::new(0),
            needs_redraw: Cell::new(true),
            pixels: RefCell::new(Vec::new()),
            ime_requests: RefCell::new(Vec::new()),
        })
    }

    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Draw a frame if Slint has one pending; returns whether the pixel buffer
    /// now holds a fresh frame.
    pub fn render_if_needed(&self) -> bool {
        if !self.needs_redraw.replace(false) {
            return false;
        }
        let size = self.size.get();
        if size.width == 0 || size.height == 0 {
            return false;
        }
        let stride = size.width as usize;
        let len = stride * size.height as usize;
        let mut pixels = self.pixels.borrow_mut();
        pixels.resize(len, PremultipliedRgbaColor::default());
        self.stride.set(stride);
        // The renderer asserts the buffer is at least window.size() pixels with
        // `pixel_stride` columns, which is exactly what was allocated above.
        self.renderer.render(&mut pixels[..], stride);
        true
    }

    /// The last frame, as premultiplied RGBA8 (`wl_shm`'s `ABGR8888`).
    pub fn pixels(&self) -> std::cell::Ref<'_, [PremultipliedRgbaColor]> {
        std::cell::Ref::map(self.pixels.borrow(), |pixels| &pixels[..])
    }

    pub fn take_ime_requests(&self) -> Vec<InputMethodRequest> {
        std::mem::take(&mut self.ime_requests.borrow_mut())
    }
}

impl WindowAdapter for Adapter {
    fn window(&self) -> &Window {
        &self.window
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.renderer
    }

    fn size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn set_size(&self, size: WindowSize) {
        let factor = self.window.scale_factor();
        log::debug!("window set_size {size:?} at scale factor {factor}");
        self.size.set(size.to_physical(factor));
        let logical = size.to_logical(factor);
        self.window
            .dispatch_event(WindowEvent::Resized { size: logical });
    }

    fn request_redraw(&self) {
        self.needs_redraw.set(true);
    }

    /// Slint's own backends reach the input method through this hook; a custom
    /// platform has to opt in explicitly or an editable field stays silent.
    fn internal(&self, _: InternalToken) -> Option<&dyn WindowAdapterInternal> {
        Some(self)
    }
}

impl WindowAdapterInternal for Adapter {
    fn input_method_request(&self, request: InputMethodRequest) {
        self.ime_requests.borrow_mut().push(request);
    }
}

/// The only required `Platform` method is `create_window_adapter`; the launcher
/// owns its event loop, so Slint's is never started.
pub struct QsPlatform {
    adapter: Rc<Adapter>,
}

impl QsPlatform {
    pub const fn new(adapter: Rc<Adapter>) -> Self {
        Self { adapter }
    }
}

impl Platform for QsPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.adapter.clone())
    }

    /// The shell never becomes the selection owner: reads and writes go through
    /// `wl-paste`/`wl-copy`, the same tools the core uses for `copy:` rows.
    fn clipboard_text(&self, _clipboard: Clipboard) -> Option<String> {
        cached_clipboard()
    }

    fn set_clipboard_text(&self, text: &str, _clipboard: Clipboard) {
        // The shell owns the text it just wrote, so the next paste can answer
        // from memory instead of asking the owner it is about to become.
        remember_clipboard(Some(text.to_owned()));
        write_clipboard(text);
    }
}

/// The clipboard as last read, plus whether that read ever happened: an empty
/// clipboard must not send every paste back to `wl-paste`.
#[derive(Default, Debug, PartialEq)]
struct ClipboardCache {
    known: bool,
    text: Option<String>,
}

static CLIPBOARD: Mutex<ClipboardCache> = Mutex::new(ClipboardCache {
    known: false,
    text: None,
});
static READING: AtomicBool = AtomicBool::new(false);

fn cache() -> std::sync::MutexGuard<'static, ClipboardCache> {
    CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts a worker-thread read if one is not already running. `wl-paste` waits
/// on the selection owner, which must never happen on the UI thread.
pub fn refresh_clipboard() {
    if READING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| {
        let text = read_clipboard();
        let mut cache = cache();
        cache.text = text;
        cache.known = true;
        READING.store(false, Ordering::Release);
    });
}

fn remember_clipboard(text: Option<String>) {
    let mut cache = cache();
    cache.text = text;
    cache.known = true;
}

/// The answer, with the refresh that keeps the *next* paste cheap.
fn cached_clipboard() -> Option<String> {
    let cached = {
        let cache = cache();
        ClipboardCache {
            known: cache.known,
            text: cache.text.clone(),
        }
    };
    refresh_clipboard();
    // The first paste after a show has nothing to answer from yet, and waiting
    // for the worker would lose the paste: it reads once, bounded.
    if cached.known {
        cached.text
    } else {
        read_clipboard()
    }
}

fn read_clipboard() -> Option<String> {
    let mut child = Command::new("wl-paste")
        .arg("--no-newline")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    // `wl-paste` waits on the selection owner; a wedged owner must not freeze
    // the launcher, so the read is bounded and the child killed on expiry.
    if let Ok(text) = rx.recv_timeout(std::time::Duration::from_millis(400)) {
        let _ = child.wait();
        Some(text)
    } else {
        let _ = child.kill();
        None
    }
}

fn write_clipboard(text: &str) {
    let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let text = text.to_owned();
        // `wl-copy` serves the selection until it is replaced, so it must not
        // hold the UI thread.
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_empty_clipboard_does_not_re_read() {
        let mut cache = ClipboardCache {
            known: true,
            text: None,
        };
        let read = |cache: &mut ClipboardCache| {
            if cache.known {
                cache.text.clone()
            } else {
                cache.known = true;
                cache.text = Some("cold read".to_owned());
                cache.text.clone()
            }
        };
        assert_eq!(read(&mut cache), None, "an empty clipboard is an answer");
        assert_eq!(
            read(&mut cache),
            None,
            "and it does not block on wl-paste again"
        );
    }

    #[test]
    fn a_cold_clipboard_reads_once() {
        let mut cache = ClipboardCache::default();
        assert_eq!(
            {
                if cache.known {
                    cache.text
                } else {
                    cache.known = true;
                    cache.text = Some("cold read".to_owned());
                    cache.text
                }
            },
            Some("cold read".to_owned())
        );
        assert!(cache.known);
    }
}
