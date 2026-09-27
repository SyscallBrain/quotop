//! The Preferences screen (`p`): the interface language, the list of themes
//! (each with a sample drawn on its own background) and the bar style. Every
//! change applies right away to the list underneath; `Enter` saves it, `Esc`
//! goes back to what was there before.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear};

use crate::i18n;
use crate::t;
use crate::tui::app::App;
use crate::tui::style;
use crate::tui::theme::{BarStyle, CATALOG};
use crate::tui::view;

/// The panel's maximum width.
const MAX_WIDTH: u16 = 72;
/// Width of the theme name column.
const NAME_WIDTH: usize = 30;
/// Width of the setting names on the language and bar rows (`Language `,
/// `Idioma `).
const SETTING_WIDTH: usize = 10;

/// Draws the panel, centered, over the list.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let inner_width = width.saturating_sub(2) as usize;
    let lines = content(app, inner_width);
    let height = (lines.len() as u16 + 3).min(area.height);
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(style::accent())
            .title(Span::styled(
                format!(" {} ", t!("tui.preferences.title")),
                style::header(),
            ))
            .style(style::base()),
        panel,
    );
    let inner = Rect::new(
        panel.x + 1,
        panel.y + 1,
        panel.width.saturating_sub(2),
        panel.height.saturating_sub(2),
    );
    let limit = inner.height.saturating_sub(1) as usize;
    for (n, line) in lines.into_iter().take(limit).enumerate() {
        frame.render_widget(line, Rect::new(inner.x, inner.y + n as u16, inner.width, 1));
    }
    let (theme, bar, language, save, cancel) = (
        t!("tui.footer.theme"),
        t!("tui.footer.bar"),
        t!("tui.footer.language"),
        t!("tui.footer.save"),
        t!("tui.footer.cancel"),
    );
    frame.render_widget(
        view::footer(
            &[
                &[
                    ("j/k", &theme),
                    ("b", &bar),
                    ("l", &language),
                    ("Enter", &save),
                    ("Esc", &cancel),
                ],
                &[
                    ("j/k", ""),
                    ("b", ""),
                    ("l", ""),
                    ("Enter", &save),
                    ("Esc", ""),
                ],
            ],
            inner.width as usize,
        ),
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

/// The panel's lines: the language, the themes, the bar and where the choice
/// is saved.
fn content(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // The languages, each by its own name, the current one marked.
    let mut languages = vec![Span::styled(
        format!(
            " {}",
            view::pad(&t!("tui.preferences.language"), SETTING_WIDTH)
        ),
        style::accent(),
    )];
    for (index, language) in i18n::LANGUAGES.iter().enumerate() {
        let chosen = index == app.preferences.language;
        languages.push(Span::styled(
            if chosen { " ▸" } else { "  " },
            style::accent(),
        ));
        languages.push(Span::styled(
            format!("{} ", language.name),
            if chosen {
                style::accent().add_modifier(Modifier::BOLD)
            } else {
                style::muted()
            },
        ));
    }
    lines.push(Line::from(languages));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        format!(" {}", t!("tui.preferences.theme")),
        style::accent(),
    )));
    for (index, theme) in CATALOG.iter().enumerate() {
        let chosen = index == app.preferences.theme;
        let (marker, name_style) = if chosen {
            ("▌", style::accent().add_modifier(Modifier::BOLD))
        } else {
            (" ", style::text())
        };
        // The sample is painted with the theme's own colors, on its own
        // background.
        let background = Style::default().bg(theme.background);
        let (filled, empty) = app.preferences.bar.render(0.65, 10);
        let mut spans = vec![
            Span::styled(format!(" {marker}"), style::accent()),
            Span::styled(view::pad(&theme.display_name(), NAME_WIDTH), name_style),
            Span::styled(" ", background),
            Span::styled("Aa ", background.fg(theme.text)),
            Span::styled("●", background.fg(theme.ok)),
            Span::styled("▲", background.fg(theme.warning)),
            Span::styled("✕ ", background.fg(theme.error)),
            Span::styled(filled, background.fg(theme.bar)),
            Span::styled(empty, background.fg(theme.track)),
            Span::styled(" ", background),
        ];
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        if used > width {
            // Too narrow for the sample: only the name stays.
            spans.truncate(2);
        }
        let line = Line::from(spans);
        lines.push(if chosen {
            line.style(style::selected_row())
        } else {
            line
        });
    }

    lines.push(Line::from(""));
    let mut bars = vec![Span::styled(
        format!(" {}", view::pad(&t!("tui.preferences.bar"), SETTING_WIDTH)),
        style::accent(),
    )];
    for bar_style in BarStyle::ALL {
        let (filled, empty) = bar_style.render(0.65, 6);
        let chosen = bar_style == app.preferences.bar;
        let label_style = if chosen {
            style::accent().add_modifier(Modifier::BOLD)
        } else {
            style::muted()
        };
        bars.push(Span::styled(
            if chosen { " ▸" } else { "  " },
            style::accent(),
        ));
        bars.push(Span::styled(format!("{} ", bar_style.id()), label_style));
        bars.push(Span::styled(
            filled,
            Style::default().fg(style::theme().bar),
        ));
        bars.push(Span::styled(
            empty,
            Style::default().fg(style::theme().track),
        ));
    }
    lines.push(Line::from(bars));

    lines.push(Line::from(""));
    let destination = match &app.preferences_path {
        Some(path) => t!("tui.preferences.saves_to", path = path.display()),
        None => t!("tui.preferences.nowhere"),
    };
    for piece in view::wrap(&destination, width.saturating_sub(2)) {
        lines.push(Line::from(Span::styled(
            format!("  {piece}"),
            style::muted(),
        )));
    }
    if !app.overrides.0.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", t!("tui.preferences.colors_note")),
            style::muted(),
        )));
    }
    lines
}
