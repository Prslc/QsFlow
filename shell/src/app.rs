//! Launcher state — the result list, its selection and the fixed five-row
//! containment window — independent of Wayland and of the Slint widgets.

use std::path::Path;
use std::rc::Rc;

use slint::{Image, Model as _, ModelRc, VecModel};

use crate::session::{Item, ThemeData};
use crate::{LauncherWindow, ResultItem, Theme};

pub const VISIBLE_ROWS: usize = 5;

/// QML-era fallbacks: a field the backend did not carry keeps its old colour
/// instead of turning transparent.
const FALLBACK: Theme = Theme {
    primary: slint::Color::from_rgb_u8(0x7a, 0xa2, 0xf7),
    on_primary: slint::Color::from_rgb_u8(0x1a, 0x1b, 0x26),
    bg: slint::Color::from_rgb_u8(0x1a, 0x1b, 0x26),
    fg: slint::Color::from_rgb_u8(0xc0, 0xca, 0xf5),
    container: slint::Color::from_rgb_u8(0x24, 0x28, 0x3b),
};

pub struct App {
    pub items: Vec<Item>,
    /// Decoded icons by row; `None` means "not asked for yet". Only the visible
    /// window is ever loaded, because every row in the model is instantiated.
    icons: Vec<Option<Image>>,
    pub selected: usize,
    pub first_row: usize,
    pub pending_forget: Option<(u64, usize)>,
    last_payload: String,
    model: Rc<VecModel<ResultItem>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            icons: Vec::new(),
            selected: 0,
            first_row: 0,
            pending_forget: None,
            last_payload: String::new(),
            model: Rc::new(VecModel::default()),
        }
    }

    pub fn install(&self, ui: &LauncherWindow) {
        ui.set_results(ModelRc::from(self.model.clone()));
    }

    /// An identical payload is dropped so a re-send cannot reset the cursor; a
    /// genuinely different one starts from the top row.
    pub fn apply_results(
        &mut self,
        ui: &LauncherWindow,
        items: Vec<Item>,
        payload: String,
    ) -> bool {
        if payload == self.last_payload && !self.items.is_empty() {
            return false;
        }
        self.last_payload = payload;
        self.items = items;
        self.icons = vec![None; self.items.len()];
        self.selected = 0;
        self.first_row = 0;
        self.model.set_vec(self.blank_rows());
        self.publish(ui);
        true
    }

    pub fn clear(&mut self, ui: &LauncherWindow) {
        self.items.clear();
        self.icons.clear();
        self.selected = 0;
        self.first_row = 0;
        self.pending_forget = None;
        self.last_payload.clear();
        self.model.set_vec(Vec::new());
        ui.set_results(ModelRc::from(self.model.clone()));
        ui.set_selected(0);
        ui.set_first_row(0);
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    /// Arrows, pages and the wheel all land here: the selection moves, and the
    /// window follows it only when it has to (the QML's `ListView.Contain`).
    pub fn move_by(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as i32 - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, last) as usize;
        self.contain();
    }

    pub fn select(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected = index;
            self.contain();
        }
    }

    pub fn contain(&mut self) {
        if self.items.is_empty() {
            self.selected = 0;
            self.first_row = 0;
            return;
        }
        if self.selected >= self.items.len() {
            self.selected = self.items.len() - 1;
        }
        if self.selected < self.first_row {
            self.first_row = self.selected;
        } else if self.selected >= self.first_row + VISIBLE_ROWS {
            self.first_row = self.selected + 1 - VISIBLE_ROWS;
        }
        self.first_row = self
            .first_row
            .min(self.items.len().saturating_sub(VISIBLE_ROWS));
    }

    /// Drops a row locally; only the core's `{"forgotten":true}` may call this.
    pub fn remove_row(&mut self, ui: &LauncherWindow, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        if index < self.icons.len() {
            self.icons.remove(index);
        }
        self.model.remove(index);
        self.contain();
        self.publish(ui);
    }

    /// Hands the visible rows to the UI, decoding any icon the window now needs.
    pub fn publish(&mut self, ui: &LauncherWindow) {
        let end = (self.first_row + VISIBLE_ROWS).min(self.items.len());
        for index in self.first_row..end {
            if self.icons[index].is_none() {
                self.icons[index] = Some(load_icon(&self.items[index].icon));
            }
            let item = &self.items[index];
            self.model.set_row_data(
                index,
                ResultItem {
                    title: item.title.clone().into(),
                    summary: item.summary.clone().into(),
                    icon: self.icons[index].clone().unwrap_or_default(),
                    on_click: item.on_click.clone().into(),
                },
            );
        }
        ui.set_selected(self.selected as i32);
        ui.set_first_row(self.first_row as i32);
    }

    fn blank_rows(&self) -> Vec<ResultItem> {
        self.items
            .iter()
            .map(|item| ResultItem {
                title: item.title.clone().into(),
                summary: item.summary.clone().into(),
                icon: Image::default(),
                on_click: item.on_click.clone().into(),
            })
            .collect()
    }
}

fn load_icon(path: &str) -> Image {
    if !path.starts_with('/') {
        return Image::default();
    }
    Image::load_from_path(Path::new(path)).unwrap_or_default()
}

/// The theme fields arrive as hex strings and keep a fallback per field.
pub fn theme_from(data: &ThemeData) -> Theme {
    Theme {
        primary: parse_hex(&data.primary).unwrap_or(FALLBACK.primary),
        on_primary: parse_hex(&data.on_primary).unwrap_or(FALLBACK.on_primary),
        bg: parse_hex(&data.bg).unwrap_or(FALLBACK.bg),
        fg: parse_hex(&data.fg).unwrap_or(FALLBACK.fg),
        container: parse_hex(&data.container).unwrap_or(FALLBACK.container),
    }
}

fn parse_hex(value: &Option<String>) -> Option<slint::Color> {
    let text = value.as_deref()?.trim().trim_start_matches('#');
    let digits = u32::from_str_radix(text, 16).ok()?;
    match text.len() {
        6 => Some(slint::Color::from_rgb_u8(
            (digits >> 16) as u8,
            (digits >> 8) as u8,
            digits as u8,
        )),
        8 => Some(slint::Color::from_argb_u8(
            (digits >> 24) as u8,
            (digits >> 16) as u8,
            (digits >> 8) as u8,
            digits as u8,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Item;

    fn app_with(count: usize) -> App {
        let mut app = App::new();
        app.items = (0..count)
            .map(|i| Item {
                title: format!("row {i}"),
                ..Default::default()
            })
            .collect();
        app.icons = vec![None; count];
        app
    }

    #[test]
    fn containment_scrolls_only_at_the_edges() {
        let mut app = app_with(10);
        app.selected = 4;
        app.contain();
        assert_eq!(app.first_row, 0, "row 4 is inside the first window");
        app.selected = 5;
        app.contain();
        assert_eq!(app.first_row, 1, "row 5 pushes the window down by one");
        app.selected = 0;
        app.contain();
        assert_eq!(app.first_row, 0);
    }

    #[test]
    fn moving_clamps_and_never_shrinks_the_window_past_the_payload() {
        let mut app = app_with(3);
        app.move_by(10);
        assert_eq!(app.selected, 2);
        assert_eq!(app.first_row, 0, "three rows fit in one window");
        app.move_by(-10);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn a_short_payload_keeps_the_window_at_zero() {
        let mut app = app_with(2);
        app.selected = 1;
        app.contain();
        assert_eq!(app.first_row, 0);
    }

    #[test]
    fn hex_parsing_accepts_hash_and_bare_forms() {
        assert_eq!(
            parse_hex(&Some("#7aa2f7".into())),
            Some(slint::Color::from_rgb_u8(0x7a, 0xa2, 0xf7))
        );
        assert_eq!(
            parse_hex(&Some("7aa2f7".into())),
            Some(slint::Color::from_rgb_u8(0x7a, 0xa2, 0xf7))
        );
        assert_eq!(parse_hex(&Some("nope".into())), None);
        assert_eq!(parse_hex(&None), None);
    }
}
