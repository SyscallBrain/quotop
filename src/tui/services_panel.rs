//! The Services & keys screen (`s`): every service in the registry, with its
//! "shown on the home screen" checkbox and the state of its key; `a` writes a
//! key to the key file.
//!
//! The text of a key being typed is **never** drawn: the screen only shows one
//! `•` per character (up to a maximum) and how many there are.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear};

use crate::i18n;
use crate::model::Category;
use crate::providers::Provider;
use crate::t;
use crate::tui::app::{App, KeyEntry};
use crate::tui::style;
use crate::tui::view::{self, category_label, pad};
use crate::tui::visibility;

/// The panel's maximum width.
const MAX_WIDTH: u16 = 88;
/// Width of the service name column.
const NAME_WIDTH: usize = 16;
/// The most `•` the entry shows (the length is shown as a number).
const MAX_DOTS: usize = 32;

/// Draws the menu over the list, taking up almost the whole screen.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let height = area.height.saturating_sub(2).max(8).min(area.height);
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
                format!(" {} ", t!("tui.services.title")),
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
    let inner_width = inner.width as usize;

    let mut top = vec![
        Line::from(Span::styled(
            view::truncate(&format!(" {}", t!("tui.services.legend")), inner_width),
            style::muted(),
        )),
        Line::from(""),
    ];
    let bottom = menu_bottom(app, inner_width);
    // What is left between the top and the bottom is the list (scrolled, if
    // it does not fit).
    let list_height = (inner.height as usize)
        .saturating_sub(top.len() + bottom.len() + 1)
        .max(1);
    let total = app.sources.providers.len();
    let (start, end) = view::window(total, app.menu.cursor, list_height);
    let category_width = category_width();
    for (index, provider) in app.sources.providers[start..end].iter().enumerate() {
        top.push(service_row(
            app,
            provider.as_ref(),
            start + index == app.menu.cursor,
            inner_width,
            category_width,
        ));
    }

    for (n, line) in top.into_iter().enumerate() {
        frame.render_widget(line, Rect::new(inner.x, inner.y + n as u16, inner.width, 1));
    }
    let bottom_y = inner.y + inner.height - bottom.len() as u16 - 1;
    for (n, line) in bottom.into_iter().enumerate() {
        frame.render_widget(
            line,
            Rect::new(inner.x, bottom_y + n as u16, inner.width, 1),
        );
    }
    let keys = if app.menu.entry.is_some() {
        let (confirm, delete, cancel) = (
            t!("tui.footer.confirm"),
            t!("tui.footer.delete"),
            t!("tui.footer.cancel"),
        );
        view::footer(
            &[
                &[
                    ("Enter", &confirm),
                    ("Backspace", &delete),
                    ("Esc", &cancel),
                ],
                &[("Enter", ""), ("Esc", &cancel)],
            ],
            inner_width,
        )
    } else {
        let (move_, space, show_hide, show, write_key, key, close) = (
            t!("tui.footer.move"),
            t!("tui.footer.space_key"),
            t!("tui.footer.show_hide"),
            t!("tui.footer.show"),
            t!("tui.footer.write_key"),
            t!("tui.footer.key"),
            t!("tui.footer.close"),
        );
        view::footer(
            &[
                &[
                    ("j/k", &move_),
                    (space.as_str(), &show_hide),
                    ("a", &write_key),
                    ("Esc", &close),
                ],
                &[(space.as_str(), &show), ("a", &key), ("Esc", &close)],
            ],
            inner_width,
        )
    };
    frame.render_widget(
        keys,
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

/// Width of the category column (the menu is the only place it appears): the
/// longest category name in the current language, plus two spaces.
fn category_width() -> usize {
    [
        Category::Search,
        Category::Llm,
        Category::Subscription,
        Category::Other,
    ]
    .into_iter()
    .map(|category| category_label(category).chars().count())
    .max()
    .unwrap_or(0)
        + 2
}

/// A menu row: cursor, checkbox, name, category and the state of the key.
fn service_row(
    app: &App,
    provider: &dyn Provider,
    on_cursor: bool,
    width: usize,
    category_width: usize,
) -> Line<'static> {
    let theme = style::theme();
    let has = app.has_credential(provider);
    let is_visible = app.visibility.is_visible(provider.id(), has);
    // A choice of the user's is one that goes against the "services with a
    // key appear" rule.
    let user_choice = is_visible != has;
    let (checkbox, checkbox_style) = if is_visible {
        ("[x] ", style::accent().add_modifier(Modifier::BOLD))
    } else {
        ("[ ] ", style::muted())
    };
    let name_style = match (on_cursor, is_visible) {
        (true, _) => style::accent().add_modifier(Modifier::BOLD),
        (false, true) => style::text(),
        (false, false) => style::muted(),
    };
    // A green `✓` means "has a key"; a muted `·` means "this key is missing".
    let (sign, key_style) = if has {
        ("✓ ", Style::default().fg(theme.ok))
    } else {
        ("· ", style::muted())
    };
    let key = if visibility::accepts_key(provider) {
        provider.variables().join(", ")
    } else if has {
        t!("tui.services.claude_session")
    } else {
        t!("tui.services.no_claude_session")
    };
    // A choice of the user's (rather than the default rule) gets a discreet
    // note.
    let note = if user_choice {
        format!("  {}", t!("tui.services.your_choice"))
    } else {
        String::new()
    };
    let rest = width.saturating_sub(2 + checkbox.len() + NAME_WIDTH + category_width + 2);
    let key = view::truncate(&key, rest.saturating_sub(note.chars().count()));
    let line = Line::from(vec![
        Span::styled(if on_cursor { " ▌" } else { "  " }, style::accent()),
        Span::styled(checkbox, checkbox_style),
        Span::styled(pad(provider.service(), NAME_WIDTH), name_style),
        Span::styled(
            pad(&category_label(provider.category()), category_width),
            style::muted(),
        ),
        Span::styled(sign, key_style),
        Span::styled(key, key_style),
        Span::styled(note, style::muted()),
    ]);
    if on_cursor {
        line.style(style::selected_row())
    } else {
        line
    }
}

/// The bottom of the menu: the key entry (if any) and the message of the last
/// action.
fn menu_bottom(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(entry) = &app.menu.entry {
        lines.extend(entry_lines(app, entry, width));
    }
    if let Some((message, error)) = &app.menu.message {
        let message_style = if *error {
            Style::default().fg(style::theme().error)
        } else {
            Style::default().fg(style::theme().ok)
        };
        for piece in view::wrap(message, width.saturating_sub(2))
            .into_iter()
            .take(2)
        {
            lines.push(Line::from(Span::styled(format!(" {piece}"), message_style)));
        }
    }
    lines
}

/// The box where the key is typed. Only `•` are drawn, never the text.
fn entry_lines(app: &App, entry: &KeyEntry, width: usize) -> Vec<Line<'static>> {
    let service = app
        .provider_by_id(&entry.provider)
        .map(|p| p.service())
        .unwrap_or("?");
    let step = if entry.variables.len() > 1 {
        format!(
            " {}",
            t!(
                "tui.services.step",
                n = entry.index + 1,
                total = entry.variables.len()
            )
        )
    } else {
        String::new()
    };
    let n = entry.text.chars().count();
    let dots = "•".repeat(n.min(MAX_DOTS));
    let count = if n == 0 {
        t!("tui.services.type_key")
    } else {
        i18n::plural("tui.services.characters", n, &[])
    };
    let destination = app
        .keys_file
        .as_ref()
        .map(|path| format!(" {}", t!("tui.services.saves_to", path = path.display())))
        .unwrap_or_default();
    vec![
        Line::from(vec![
            Span::styled(
                format!(" {}", t!("tui.services.key_for", service = service)),
                style::text(),
            ),
            Span::styled(
                format!("{}{step}", entry.variable()),
                style::accent().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" › ", style::accent()),
            Span::styled(dots, style::text()),
            Span::styled("▏", style::accent()),
            Span::styled(format!("  {count}"), style::muted()),
        ])
        .style(style::selected_row()),
        Line::from(Span::styled(
            view::truncate(&destination, width),
            style::muted(),
        )),
    ]
}
