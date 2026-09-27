//! The screen styles, taken from the theme in use.
//!
//! The theme lives in the [`App`](crate::tui::App); every frame starts by
//! setting it here with [`set_current`], so the drawing functions can ask for
//! styles without receiving the theme as an argument. It is per thread (all
//! drawing happens on the main thread) so tests running in parallel do not
//! step on each other.

use std::cell::Cell;

use ratatui::style::{Color, Modifier, Style};

use crate::model::Level;
use crate::tui::theme::Theme;

thread_local! {
    static CURRENT: Cell<Theme> = const { Cell::new(Theme::default_theme()) };
}

/// Sets the theme for the drawing that follows on this thread.
pub fn set_current(theme: Theme) {
    CURRENT.with(|current| current.set(theme));
}

/// The theme in use.
pub fn theme() -> Theme {
    CURRENT.with(Cell::get)
}

/// The style of everything without a color of its own.
pub fn base() -> Style {
    let t = theme();
    Style::default().fg(t.text).bg(t.background)
}

/// The screen title: the `quotop` badge in reverse, in the accent color.
pub fn badge() -> Style {
    let t = theme();
    Style::default()
        .fg(t.background)
        .bg(t.accent)
        .add_modifier(Modifier::BOLD)
}

/// Titles (of the detail, help and preferences): bold accent.
pub fn header() -> Style {
    Style::default()
        .fg(theme().accent)
        .add_modifier(Modifier::BOLD)
}

/// Notes, help and footer: secondary text, readable without competing with
/// the data.
pub fn muted() -> Style {
    Style::default().fg(theme().muted)
}

/// Borders and separators.
pub fn border() -> Style {
    Style::default().fg(theme().border)
}

/// Section titles and key hints.
pub fn accent() -> Style {
    Style::default().fg(theme().accent)
}

/// Normal text (no background, so it inherits the row's).
pub fn text() -> Style {
    Style::default().fg(theme().text)
}

/// The selected row: only the background changes, so the traffic-light
/// colors stay.
pub fn selected_row() -> Style {
    Style::default().bg(theme().selection)
}

/// The traffic-light color: ok, warning, critical/depleted, and the muted
/// color of meters with no reference to compare against.
pub fn level_color(level: Level) -> Color {
    let t = theme();
    match level {
        Level::Ok => t.ok,
        Level::Warning => t.warning,
        Level::Critical | Level::Exhausted => t.error,
        Level::NoReference => t.muted,
    }
}

/// The color of the filled part of a bar: the theme's bar color when all is
/// well, the traffic-light color when it is not.
pub fn bar_color(level: Level) -> Color {
    match level {
        Level::Ok | Level::NoReference => theme().bar,
        other => level_color(other),
    }
}

/// The traffic-light symbol, one column wide.
pub fn symbol(level: Level) -> char {
    match level {
        Level::Ok => '●',
        Level::Warning => '▲',
        Level::Critical | Level::Exhausted => '✕',
        Level::NoReference => '○',
    }
}
