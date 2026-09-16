//! The view: a dimmed backdrop with a card fixed in the upper half of the
//! screen. Ported from `ui/SearchWindow.qml` / `ui/ResultDelegate.qml`.

use iced::font::Weight;
use iced::widget::canvas;
use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, Space, container, image, mouse_area, svg, text, text_input};
use iced::window::Id as WindowId;
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Font, Length, Padding, Point,
    Rectangle, Renderer, Size, Theme as IcedTheme, mouse,
};

use crate::app::{Hover, INPUT_ID, Message, State};
use crate::geometry;

/// Text/icon size defaults of the QML.
const TITLE_SIZE: f32 = 14.0;
const SUMMARY_SIZE: f32 = 12.0;
const SUGGESTION_SIZE: f32 = 11.0;

pub fn view(state: &State, _window: WindowId) -> Element<'_, Message> {
    let surface = state.surface;

    let dim = solid(Color::BLACK, geometry::DIM_ALPHA * state.dim_alpha());
    // Enter/exit only gate the wheel; the press is what keeps a click inside the
    // card from dismissing.
    let card = mouse_area(card(state, surface))
        .on_press(Message::SwallowClick)
        .on_enter(Message::PointerOnCard(true))
        .on_exit(Message::PointerOnCard(false));

    let backdrop = container(card)
        .width(Length::Fixed(surface.width))
        .height(Length::Fixed(surface.height))
        .padding(Padding {
            top: geometry::card_top(surface),
            left: geometry::card_x(surface),
            right: surface.width - geometry::card_x(surface) - geometry::card_w(surface),
            bottom: 0.0,
        })
        .style(move |_theme| container::Style {
            background: Some(Background::Color(dim)),
            ..container::Style::default()
        });

    // a press on the backdrop — never on the card, which swallows its own
    mouse_area(backdrop).on_press(Message::Dismiss).into()
}

fn card(state: &State, surface: Size) -> Element<'_, Message> {
    let theme = &state.theme;
    let fill = state.fade(theme.container, geometry::CARD_ALPHA);

    let mut content = Column::new().spacing(geometry::GAP).push(search_row(state));

    if !state.rows.is_empty() {
        content = content.push(list(state));
    }

    content = content.push(footer(state));

    container(content)
        .width(Length::Fixed(geometry::card_w(surface)))
        .height(Length::Fixed(state.card_height()))
        // During the 150ms reflow the column is already at its final height while
        // the card animates towards it, so a growing payload would paint the last
        // rows below the card's edge. The QML clipped the card for this reason.
        .clip(true)
        .padding(geometry::PAD)
        .style(move |_theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                // a hairline that masks the rounded-rect AA rim
                color: state.fade(Color::WHITE, 0.35),
                width: 1.0,
                radius: geometry::RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn search_row(state: &State) -> Element<'_, Message> {
    let theme = &state.theme;
    let accent = theme.primary;
    let fg = theme.fg;
    let dim = state.fade(fg, 0.55);

    let magnifier = canvas(Magnifier { color: dim }).width(22.0).height(22.0);

    let input = text_input("Search apps, files, web...", &state.query)
        .id(INPUT_ID)
        .on_input(Message::Typed)
        .on_submit(Message::Submit)
        .padding(8)
        .size(18)
        // Medium (not Bold): the typed query gains a little emphasis over the
        // hint without the full weight, which read as too heavy at 18px — and
        // iced shares one font between the value and the placeholder, so Bold
        // put the dimmed hint in a bold face as well.
        .font(Font {
            weight: Weight::Medium,
            ..Font::DEFAULT
        })
        .style(move |_theme, _status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: dim,
            placeholder: dim,
            value: state.fade(fg, 1.0),
            selection: state.fade(accent, 0.35),
        });

    let mut toolbar = Row::new().spacing(6).align_y(Alignment::Center);
    if let Some(prefix) = keyword_prefix(&state.query) {
        toolbar = toolbar.push(
            container(
                text(prefix)
                    .size(SUGGESTION_SIZE)
                    .font(Font {
                        weight: Weight::Bold,
                        ..Font::DEFAULT
                    })
                    .color(state.fade(accent, 1.0)),
            )
            .height(24)
            .center_y(24)
            .padding(Padding {
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
                left: 8.0,
            })
            .style(move |_theme| container::Style {
                background: Some(Background::Color(state.fade(accent, 0.16))),
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
        );
    }
    if !state.query.is_empty() {
        toolbar = toolbar.push(clear_button(state, dim));
    }

    let row = Row::new()
        .spacing(12)
        .align_y(Alignment::Center)
        .push(magnifier)
        .push(input.width(Length::Fill))
        .push(toolbar);

    container(row)
        .height(geometry::SEARCH_H)
        .align_y(Alignment::Center)
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 14.0,
        })
        .style(move |_theme| container::Style {
            background: Some(Background::Color(state.fade(fg, 0.08))),
            border: Border {
                radius: 9.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn clear_button(state: &State, dim: Color) -> Element<'_, Message> {
    let hovered = state.hovered == Some(Hover::Clear);
    // A soft disc rather than a 1px outline: a thin ring around a 26px circle
    // reads as a scratchy hairline, while the tinted disc gives the ✕ the same
    // "this is a button" affordance quietly (and deepens on hover).
    let background = state.fade(state.theme.fg, if hovered { 0.18 } else { 0.10 });

    mouse_area(
        container(text("✕").size(12).color(dim))
            // The glyph's ink sits 1.5px above and 0.5px left of its line box, so
            // centring the box leaves the ✕ visibly high inside the disc (invisible
            // while the button had no fill). A 3px top / 1px left padding shifts the
            // centred box back by half of each, putting the ink on the disc's centre.
            .padding(Padding {
                top: 3.0,
                right: 0.0,
                bottom: 0.0,
                left: 1.0,
            })
            .width(26)
            .height(26)
            .center_x(26)
            .center_y(26)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    radius: 13.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
    )
    .on_enter(Message::Hover(Hover::Clear))
    .on_exit(Message::Unhover(Hover::Clear))
    .on_press(Message::ClearQuery)
    .into()
}

fn list(state: &State) -> Element<'_, Message> {
    // Only the visible window is built: `State::first` is the whole scroll
    // state, so a page change is a repaint rather than an operation the runtime
    // applies one dispatch later (which also never requested a redraw).
    let rows = Column::new().width(Length::Fill).extend(
        state
            .rows
            .iter()
            .enumerate()
            .skip(state.first)
            .take(geometry::MAX_ROWS)
            .map(|(index, row)| row_view(state, index, row)),
    );

    container(rows)
        .height(Length::Fixed(geometry::list_h(state.rows.len())))
        .into()
}

fn row_view<'a>(state: &'a State, index: usize, row: &'a crate::app::Row) -> Element<'a, Message> {
    let theme = &state.theme;
    let accent = theme.primary;
    let selected = index == state.selected;
    let hovered = state.hovered == Some(Hover::Row(index));
    let background = match (selected, hovered) {
        (true, _) => state.fade(accent, 0.15),
        (false, true) => state.fade(accent, 0.08),
        (false, false) => Color::TRANSPARENT,
    };

    // Always 3px wide so the icon sits at the same x on every row; only the
    // selected row paints it.
    let bar = container(Space::new())
        .width(3.0)
        .height(28.0)
        .style(move |_theme| container::Style {
            background: selected.then(|| Background::Color(state.fade(accent, 1.0))),
            border: Border {
                radius: 1.5.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    let labels = {
        let mut labels = Column::new().spacing(2).width(Length::Fill).push(
            text(&row.title)
                .size(TITLE_SIZE)
                .font(Font {
                    weight: Weight::Bold,
                    ..Font::DEFAULT
                })
                .wrapping(Wrapping::None)
                .color(state.fade(theme.fg, 1.0)),
        );

        if let Some(summary) = row.summary.as_deref() {
            labels = labels.push(
                text(summary)
                    .size(SUMMARY_SIZE)
                    .wrapping(Wrapping::None)
                    .color(state.fade(theme.fg, 0.7)),
            );
        }

        labels
    };

    // Explicit gaps, not one uniform spacing: the QML puts the icon 11px from
    // the row's left edge (bar 3 + gap 5) and the labels 12px after the 30px
    // icon.
    let mut content = Row::new()
        .spacing(0)
        .align_y(Alignment::Center)
        .push(bar)
        .push(Space::new().width(5.0))
        .push(icon(row.icon_path.as_deref(), state.entrance_alpha()))
        .push(Space::new().width(12.0))
        .push(container(labels).width(Length::Fill).clip(true));

    if selected {
        content = content
            .push(Space::new().width(12.0))
            .push(text("↵").size(13).color(state.fade(accent, 0.55)));
    }

    mouse_area(
        container(content)
            .width(Length::Fill)
            .height(geometry::ROW_H)
            .align_y(Alignment::Center)
            .padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 3.0,
            })
            .style(move |_theme| container::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    radius: 8.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
    )
    .on_enter(Message::Hover(Hover::Row(index)))
    .on_exit(Message::Unhover(Hover::Row(index)))
    .on_press(Message::RowClick(index))
    .into()
}

/// Absolute paths render directly (the core resolves every other spec).
fn icon(path: Option<&str>, fade: f32) -> Element<'_, Message> {
    const SIZE: f32 = 30.0;

    let Some(path) = path else {
        return Space::new().width(SIZE).height(SIZE).into();
    };

    if path.to_ascii_lowercase().ends_with(".svg") {
        svg::Svg::from_path(path)
            .width(SIZE)
            .height(SIZE)
            .content_fit(ContentFit::Contain)
            .opacity(fade)
            .into()
    } else {
        image(image::Handle::from_path(path))
            .width(SIZE)
            .height(SIZE)
            .content_fit(ContentFit::Contain)
            .opacity(fade)
            .into()
    }
}

fn footer(state: &State) -> Element<'_, Message> {
    let theme = &state.theme;
    let empty = state.rows.is_empty();

    let hints = if empty {
        "Type ? for help  ·  prefixes: b h f d c s g tr r w"
    } else {
        "↵ Launch   ↑↓ Move   ⌫ Forget   Esc Close"
    };
    let count = if empty {
        String::new()
    } else {
        format!("{} results", state.rows.len())
    };

    Row::new()
        .height(geometry::FOOTER_H)
        .align_y(Alignment::Center)
        .push(
            text(hints)
                .size(SUGGESTION_SIZE)
                .color(state.fade(theme.fg, 0.5)),
        )
        .push(Space::new().width(Length::Fill))
        .push(
            text(count)
                .size(SUGGESTION_SIZE)
                .color(state.fade(theme.fg, 0.45)),
        )
        .into()
}

/// The active keyword prefix (`b`, `h`, `f`, …): `^([a-zA-Z]{1,3})\s`.
///
/// Pure frontend derivation — the core has no prefix table.
fn keyword_prefix(query: &str) -> Option<&str> {
    let end = query
        .as_bytes()
        .iter()
        .take(4)
        .position(|byte| *byte == b' ')?;
    if end == 0 || !query.as_bytes()[..end].iter().all(u8::is_ascii_alphabetic) {
        return None;
    }

    Some(&query[..end])
}

/// A fully opaque `color` at the given `alpha` (the QML's `Qt.alpha`).
fn solid(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// The magnifier glyph, drawn rather than loaded: a 2px circle of radius 6.2 at
/// (9, 9) with a handle to (19, 19) in a 22×22 box.
struct Magnifier {
    color: Color,
}

impl canvas::Program<Message, IcedTheme, Renderer> for Magnifier {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &IcedTheme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let stroke = canvas::Stroke::default()
            .with_color(self.color)
            .with_width(2.0);

        frame.stroke(&canvas::Path::circle(Point::new(9.0, 9.0), 6.2), stroke);
        frame.stroke(
            &canvas::Path::line(Point::new(13.8, 13.8), Point::new(19.0, 19.0)),
            stroke,
        );

        vec![frame.into_geometry()]
    }
}
