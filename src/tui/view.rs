//! Drawing the screen: header, list box, notes, footer and help. Nothing here
//! talks to the outside world — it takes the [`App`] and writes to the
//! `Frame`.
//!
//! The list box is drawn row by row, not with a `Block`: the top border
//! carries a title, separators are rows with text, and each data row has its
//! own colors. Knowing each column's width is what lets text that does not
//! fit be truncated instead of spilling off the screen — and `†`, `$`, `█` and
//! `·` are all one column wide, so character widths never need measuring.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::i18n;
use crate::model::{Category, Cost, Level};
use crate::t;
use crate::tui::app::{self, App, Counts, MIN_HEIGHT, MIN_WIDTH, Row};
use crate::tui::detail;
use crate::tui::preferences_panel;
use crate::tui::services_panel;
use crate::tui::style;
use crate::tui::theme::BarStyle;

/// The column of the selected-row marker (`▌`).
const MARKER_WIDTH: usize = 1;
/// Width of the name column: the longest name (`Brave Search`), plus the two
/// characters of a marker (` $` or ` †`) and a space before the label.
const NAME_WIDTH: usize = 15;
/// Width of the meter label column (`monthly plan` and a space).
const LABEL_WIDTH: usize = 13;
/// The label column from [`WIDE_FROM`] columns on, so that longer labels
/// (`requests/month`) fit whole when there is room for them.
const WIDE_LABEL_WIDTH: usize = 15;
/// Inner width from which the label column gets [`WIDE_LABEL_WIDTH`].
const WIDE_FROM: usize = 88;
/// Width of the traffic-light column: a separating space, the symbol, another
/// space and a level label of up to 8 characters.
const LEVEL_WIDTH: usize = 11;
/// The spaces between the bar (or the percentage) and the values.
const GAP: usize = 2;
/// The space before the right border.
const PADDING: usize = 1;
/// The remaining-percentage column (` 100%`), when it fits.
const PERCENT_WIDTH: usize = 5;
/// Below this height there is no room for the column titles.
const HEIGHT_WITH_TITLES: u16 = 16;

/// The widths of the list columns, for a given inner width.
struct Columns {
    name: usize,
    label: usize,
    bar: usize,
    percent: usize,
    values: usize,
    level: usize,
    bar_style: BarStyle,
}

impl Columns {
    /// Splits what is left after the fixed columns between the bar, the
    /// percentage and the values. The values get what they need (up to 30),
    /// the bar gets the rest (up to 20) and the percentage only appears from
    /// about 90 columns — below that, the bar already says the same thing.
    fn for_width(inner: usize, widest_value: usize, bar_style: BarStyle) -> Columns {
        let label = if inner >= WIDE_FROM {
            WIDE_LABEL_WIDTH
        } else {
            LABEL_WIDTH
        };
        let rest =
            inner.saturating_sub(MARKER_WIDTH + NAME_WIDTH + label + GAP + LEVEL_WIDTH + PADDING);
        let percent = if rest >= 44 { PERCENT_WIDTH } else { 0 };
        let free = rest - percent;
        let min_bar = match free {
            0..24 => 6,
            24..40 => 8,
            _ => 10,
        };
        let values = widest_value.clamp(16, 30).min(free.saturating_sub(min_bar));
        let bar = free.saturating_sub(values).min(20);
        Columns {
            name: NAME_WIDTH,
            label,
            values: free - bar,
            bar,
            percent,
            level: LEVEL_WIDTH,
            bar_style,
        }
    }
}

/// Draws the whole screen. The language and the theme come first (every text
/// and style depends on them), and the background is painted next, so the
/// screen does not inherit the terminal's colors.
pub fn draw(frame: &mut Frame, app: &App) {
    i18n::set_current(app.preferences.language);
    style::set_current(app.theme());
    let area = frame.area();
    frame.render_widget(
        ratatui::widgets::Block::default().style(style::base()),
        area,
    );
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        too_small(frame, area);
        return;
    }
    match app.view {
        app::View::List => list(frame, area, app),
        app::View::Detail => {
            // The detail is an overlay: the list stays underneath.
            list(frame, area, app);
            detail::draw(frame, area, app);
        }
        app::View::Preferences => {
            // The list stays underneath so the theme and language changes can
            // be seen live.
            list(frame, area, app);
            preferences_panel::draw(frame, area, app);
        }
        app::View::Services => {
            list(frame, area, app);
            services_panel::draw(frame, area, app);
        }
        app::View::Help => help(frame, area, app),
    }
}

/// Below 60×12 nothing readable fits: only the message.
fn too_small(frame: &mut Frame, area: Rect) {
    let y = area.y + area.height / 2;
    frame.render_widget(
        Line::from(Span::styled(t!("tui.too_small"), style::base())).alignment(Alignment::Center),
        Rect::new(area.x, y, area.width, 1),
    );
}

/// The list of services with the header, column titles, notes and footer.
pub fn list(frame: &mut Frame, area: Rect, app: &App) {
    let rows = app.rows();
    let width = area.width as usize;
    let columns = Columns::for_width(
        width.saturating_sub(2),
        widest_primary(&rows),
        app.preferences.bar,
    );
    let selected = app.selected_index(&rows);
    let notes = wrap_all(&app.notes().join(" · "), width.saturating_sub(2));

    // Header, column titles, notes and footer get what they need; the box
    // gets the rest — or only what the rows need, when there are few.
    let with_titles = area.height >= HEIGHT_WITH_TITLES;
    let top = 1 + u16::from(with_titles);
    let notes_height = notes.len().min(2) as u16;
    let box_height = area
        .height
        .saturating_sub(top + 1 + notes_height)
        .min(rows.len().max(1) as u16 + 2)
        .max(3);

    frame.render_widget(header(app, width), Rect::new(area.x, area.y, area.width, 1));
    if with_titles {
        frame.render_widget(
            column_titles(&columns),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
    }
    list_box(
        frame,
        Rect::new(area.x, area.y + top, area.width, box_height),
        &rows,
        selected,
        &columns,
        width,
    );
    for (n, note) in notes.iter().take(2).enumerate() {
        let y = area.y + top + box_height + n as u16;
        frame.render_widget(
            Line::from(Span::styled(format!(" {note}"), style::muted())),
            Rect::new(area.x, y, area.width, 1),
        );
    }
    let (move_, detail, read_row, read_all, read, filter, services, preferences, prefs, help, quit) = (
        t!("tui.footer.move"),
        t!("tui.footer.detail"),
        t!("tui.footer.read_row"),
        t!("tui.footer.read_all"),
        t!("tui.footer.read"),
        t!("tui.footer.filter"),
        t!("tui.footer.services"),
        t!("tui.footer.preferences"),
        t!("tui.footer.prefs"),
        t!("tui.footer.help"),
        t!("tui.footer.quit"),
    );
    frame.render_widget(
        footer(
            &[
                &[
                    ("j/k", &move_),
                    ("Enter", &detail),
                    ("r", &read_row),
                    ("R", &read_all),
                    ("f", &filter),
                    ("s", &services),
                    ("p", &preferences),
                    ("?", &help),
                    ("q", &quit),
                ],
                &[
                    ("j/k", &move_),
                    ("Enter", &detail),
                    ("r/R", &read),
                    ("s", &services),
                    ("p", &prefs),
                    ("?", &help),
                    ("q", &quit),
                ],
                &[
                    ("↵", ""),
                    ("r/R", &read),
                    ("s", &services),
                    ("p", &prefs),
                    ("?", ""),
                    ("q", &quit),
                ],
            ],
            width,
        ),
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
    );
}

/// The column titles, aligned with the box.
fn column_titles(columns: &Columns) -> Line<'static> {
    let title_style = style::muted().add_modifier(Modifier::BOLD);
    let mut text = " ".repeat(1 + MARKER_WIDTH);
    text.push_str(&pad(&t!("tui.columns.service"), columns.name));
    text.push_str(&pad(&t!("tui.columns.meter"), columns.label));
    text.push_str(&pad(
        &t!("tui.columns.left"),
        columns.bar + columns.percent + GAP,
    ));
    text.push_str(&pad(&t!("tui.columns.value"), columns.values));
    text.push_str(&format!(
        "{:>width$}",
        t!("tui.columns.level"),
        width = columns.level
    ));
    Line::from(Span::styled(text, title_style))
}

/// The list box: the top border with the title, the rows and the bottom
/// border (with the position, when the list does not fit).
fn list_box(
    frame: &mut Frame,
    area: Rect,
    rows: &[Row],
    selected: Option<usize>,
    columns: &Columns,
    width: usize,
) {
    let height = area.height.saturating_sub(2) as usize;
    let selected_or_top = selected.unwrap_or(0);
    let (start, end) = window(rows.len(), selected_or_top, height);
    let title = i18n::plural("tui.list.services", count_services(rows), &[]);
    frame.render_widget(
        titled_border(('╭', '╮'), &title, width),
        Rect::new(area.x, area.y, area.width, 1),
    );
    if rows.is_empty() {
        // Nothing to show: say how to get something, instead of an empty box.
        let space = width.saturating_sub(2);
        frame.render_widget(
            Line::from(vec![
                Span::styled("│", style::border()),
                Span::styled(
                    pad(&format!("  {}", t!("tui.list.empty")), space),
                    style::muted(),
                ),
                Span::styled("│", style::border()),
            ]),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
    }
    for (n, row) in rows[start..end].iter().enumerate() {
        let y = area.y + 1 + n as u16;
        let is_selected = selected == Some(start + n);
        frame.render_widget(
            data_row(row, columns, is_selected, width),
            Rect::new(area.x, y, area.width, 1),
        );
    }

    // The position only appears when some rows are outside the window.
    let position = if end - start < rows.len() {
        format!(
            " {} ",
            t!(
                "tui.list.position",
                n = selected_or_top + 1,
                total = rows.len()
            )
        )
    } else {
        String::new()
    };
    let fill = width.saturating_sub(position.chars().count() + 3);
    frame.render_widget(
        Line::from(vec![
            Span::styled(format!("╰{}", "─".repeat(fill)), style::border()),
            Span::styled(position, style::muted()),
            Span::styled("─╯", style::border()),
        ]),
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
    );
}

/// How many services have rows (a service with two meters counts once).
fn count_services(rows: &[Row]) -> usize {
    rows.iter().filter(|row| row.first).count()
}

/// A data row: marker, name (only on the service's first row), label, bar of
/// the remaining fraction, percentage, values and traffic light.
fn data_row(row: &Row, columns: &Columns, is_selected: bool, width: usize) -> Line<'static> {
    let theme = style::theme();
    let no_data = row.is_status() && !row.failed;
    let name_style = if is_selected {
        style::accent().add_modifier(Modifier::BOLD)
    } else if no_data {
        style::muted()
    } else {
        style::text()
    };
    let marker = if is_selected { "▌" } else { " " };
    let mut spans = vec![
        Span::styled("│", style::border()),
        Span::styled(marker, style::accent()),
        Span::styled(row_name(row, columns.name), name_style),
    ];
    if row.is_status() {
        // The status text spans from the label to the values; the traffic
        // light only says something when the reading failed.
        let space = width.saturating_sub(2 + MARKER_WIDTH + columns.name + columns.level + PADDING);
        let (status_style, sign) = if row.failed {
            (Style::default().fg(theme.error), t!("tui.list.failed"))
        } else {
            (style::muted(), String::new())
        };
        spans.push(Span::styled(pad(&row.primary, space), status_style));
        spans.push(Span::styled(
            format!(
                "{:>width$}",
                truncate(&sign, columns.level),
                width = columns.level
            ),
            status_style,
        ));
        return close_row(spans, is_selected);
    }

    let level = row.level.unwrap_or(Level::NoReference);
    let level_color = style::level_color(level);
    spans.push(Span::styled(
        pad(&i18n::label(&row.label), columns.label),
        style::muted(),
    ));
    match row.bar {
        Some(fraction) => {
            let (filled, empty) = columns.bar_style.render(fraction, columns.bar);
            spans.push(Span::styled(
                filled,
                Style::default().fg(style::bar_color(level)),
            ));
            spans.push(Span::styled(empty, Style::default().fg(theme.track)));
            if columns.percent > 0 {
                let percent = format!("{}%", (fraction * 100.0).round() as u32);
                let percent_style = if level == Level::Ok {
                    style::muted()
                } else {
                    Style::default().fg(level_color)
                };
                spans.push(Span::styled(
                    format!("{percent:>width$}", width = columns.percent),
                    percent_style,
                ));
            }
        }
        None => {
            // With no limit to compare against there is no fraction: a light
            // dashed line, which cannot be mistaken for an empty bar.
            spans.push(Span::styled("╌".repeat(columns.bar), style::border()));
            if columns.percent > 0 {
                spans.push(Span::styled(
                    format!("{:>width$}", "—", width = columns.percent),
                    style::muted(),
                ));
            }
        }
    }
    spans.push(Span::raw(" ".repeat(GAP)));
    let mut value = row.primary.clone();
    let room = columns.values.saturating_sub(value.chars().count());
    // The "reset …" only goes in when it fits whole: a cut "reset 5h 3…"
    // tells nothing (the detail always has it).
    let mut extra = String::new();
    if !row.extra.is_empty() {
        let candidate = format!(" · {}", row.extra);
        if room >= candidate.chars().count() {
            extra = candidate;
        }
    }
    value = truncate(&value, columns.values.saturating_sub(extra.chars().count()));
    let room = columns.values - value.chars().count() - extra.chars().count();
    spans.push(Span::styled(value, style::text()));
    spans.push(Span::styled(extra, style::muted()));
    spans.push(Span::raw(" ".repeat(room)));

    // The traffic light: `ok` is discreet (the symbol has the color, the text
    // does not); the rest is colored end to end — what needs attention stands
    // out.
    let label = truncate(&level_label(level), columns.level.saturating_sub(2));
    let symbol = style::symbol(level);
    let indent = columns.level.saturating_sub(label.chars().count() + 2);
    spans.push(Span::raw(" ".repeat(indent)));
    spans.push(Span::styled(
        format!("{symbol} "),
        Style::default().fg(level_color),
    ));
    let label_style = match level {
        Level::Ok | Level::NoReference => style::muted(),
        Level::Warning => Style::default().fg(level_color),
        Level::Critical | Level::Exhausted => Style::default()
            .fg(level_color)
            .add_modifier(Modifier::BOLD),
    };
    spans.push(Span::styled(label, label_style));
    close_row(spans, is_selected)
}

/// Closes the row with the padding and the right border, and applies the
/// selection background (it has to cover the whole row, not just the text).
fn close_row(mut spans: Vec<Span<'static>>, is_selected: bool) -> Line<'static> {
    spans.push(Span::raw(" ".repeat(PADDING)));
    spans.push(Span::styled("│", style::border()));
    let line = Line::from(spans);
    if is_selected {
        line.style(style::selected_row())
    } else {
        line
    }
}

/// The service name, with its markers: `$` for services paid per request and
/// `†` for undocumented endpoints.
fn row_name(row: &Row, width: usize) -> String {
    if !row.first {
        return " ".repeat(width);
    }
    let mut name = row.service.clone();
    if row.cost == Cost::PerRequest {
        name.push_str(" $");
    }
    if row.undocumented {
        name.push_str(" †");
    }
    pad(&name, width)
}

/// A horizontal border with a title (the top border and the separators).
fn titled_border((left, right): (char, char), title: &str, width: usize) -> Line<'static> {
    let fill = width.saturating_sub(title.chars().count() + 5);
    Line::from(vec![
        Span::styled(format!("{left}─"), style::border()),
        Span::styled(
            format!(" {title} "),
            style::accent().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{}{right}", "─".repeat(fill)), style::border()),
    ])
}

/// The header: the badge, the summary per level (only what exists) and, on
/// the right, the number of services, the age of the readings and what comes
/// next.
fn header(app: &App, width: usize) -> Line<'static> {
    let theme = style::theme();
    let mut spans = vec![Span::styled(" quotop ", style::badge()), Span::raw(" ")];
    let Counts {
        critical,
        warnings,
        ok,
        failures,
        no_data,
    } = app.counts();
    let chips = [
        (critical, "✕", "tui.header.critical", theme.error),
        (warnings, "▲", "tui.header.warning", theme.warning),
        (failures, "!", "tui.header.failed", theme.error),
        (ok, "●", "tui.header.ok", theme.ok),
        (no_data, "○", "tui.header.no_data", theme.muted),
    ];
    for (n, symbol, key, color) in chips {
        if n == 0 {
            continue;
        }
        spans.push(Span::styled(
            format!("{symbol} "),
            Style::default().fg(color),
        ));
        spans.push(Span::styled(
            format!("{}  ", i18n::plural(key, n, &[])),
            style::text(),
        ));
    }
    let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    let space = width.saturating_sub(used + 1);
    // On the right, the most complete version that fits: the service count
    // goes first (the chips already add up to it), then the next refresh; the
    // age of the readings stays to the end, truncated if needed.
    let services = i18n::plural("tui.list.services", app.visible_providers().len(), &[]);
    let (age, refresh_state) = (app.age(), app.refresh_state());
    let candidates = [
        format!("{services} · {age} · {refresh_state} "),
        format!("{age} · {refresh_state} "),
        format!("{age} "),
        // Just "3m ago": it reads fine without the "updated".
        format!("{} ", app.age_short()),
    ];
    let right = candidates
        .iter()
        .find(|text| text.chars().count() <= space)
        .cloned()
        .unwrap_or_else(|| truncate(&candidates[3], space));
    // Fewer than 12 columns is just noise.
    if space >= 12 {
        let indent = space + 1 - right.chars().count();
        spans.push(Span::raw(" ".repeat(indent)));
        spans.push(Span::styled(right, style::muted()));
    }
    Line::from(spans).style(style::base())
}

/// The footer with the view's key hints: uses the most complete of the
/// `candidates` that fits the width (from longest to shortest) — at 60
/// columns the hints get shorter, but `q quit` never disappears.
pub fn footer(candidates: &[&[(&str, &str)]], width: usize) -> Line<'static> {
    let measure = |keys: &[(&str, &str)]| -> usize {
        keys.iter()
            .map(|(key, meaning)| {
                key.chars().count() + meaning.chars().count() + usize::from(!meaning.is_empty())
            })
            .sum::<usize>()
            + 3 * keys.len().saturating_sub(1)
            + 1
    };
    let keys = candidates
        .iter()
        .find(|keys| measure(keys) <= width)
        .or_else(|| candidates.last())
        .copied()
        .unwrap_or(&[]);
    let mut spans = vec![Span::raw(" ")];
    for (n, (key, meaning)) in keys.iter().enumerate() {
        if n > 0 {
            spans.push(Span::styled(" · ", style::border()));
        }
        spans.push(Span::styled(
            key.to_string(),
            style::accent().add_modifier(Modifier::BOLD),
        ));
        if !meaning.is_empty() {
            spans.push(Span::styled(format!(" {meaning}"), style::muted()));
        }
    }
    Line::from(spans)
}

/// The help: key bindings, markers, levels, preferences and this run's notes
/// (in full — this is where they fit).
fn help(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(" quotop ", style::badge()),
        Span::styled(format!("  {}", t!("tui.help.title")), style::header()),
    ]));
    let bindings = [
        ("j/k, ↓/↑", t!("tui.help.move")),
        ("g/G", t!("tui.help.first_last")),
        ("Enter, l", t!("tui.help.open_detail")),
        ("Esc, h", t!("tui.help.back")),
        ("r", t!("tui.help.read_row")),
        ("R", t!("tui.help.read_all")),
        ("f", t!("tui.help.filter")),
        ("s", t!("tui.help.services")),
        ("p", t!("tui.help.preferences")),
        ("?", t!("tui.help.this_help")),
        ("q, Ctrl+C", t!("tui.help.quit")),
    ];
    for (key, meaning) in bindings {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<11}"),
                style::accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled(meaning, style::base()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(section_title(t!("tui.help.markers")));
    lines.push(Line::from(Span::styled(
        format!("  {}", t!("tui.help.per_request")),
        style::base(),
    )));
    lines.push(Line::from(Span::styled(
        format!("  {}", t!("tui.help.undocumented")),
        style::base(),
    )));
    let (filled, empty) = app.preferences.bar.render(0.6, 10);
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(filled, Style::default().fg(style::theme().bar)),
        Span::styled(empty, Style::default().fg(style::theme().track)),
        Span::styled(format!("  {}", t!("tui.help.bar")), style::base()),
    ]));
    lines.push(Line::from(""));
    lines.push(section_title(t!("tui.help.levels")));
    let mut levels = vec![Span::raw(" ")];
    for level in [
        Level::Ok,
        Level::Warning,
        Level::Critical,
        Level::Exhausted,
        Level::NoReference,
    ] {
        levels.push(Span::styled(
            format!(" {} ", style::symbol(level)),
            Style::default().fg(style::level_color(level)),
        ));
        levels.push(Span::styled(level_label(level), style::base()));
        levels.push(Span::raw(" "));
    }
    lines.push(Line::from(levels));
    lines.push(Line::from(Span::styled(
        format!("  {}", t!("tui.help.no_reference")),
        style::muted(),
    )));
    lines.push(Line::from(""));
    lines.push(section_title(t!("tui.help.preferences_title")));
    lines.push(Line::from(Span::styled(
        format!(
            "  {}",
            t!(
                "tui.help.preferences_now",
                language = app.preferences.language().name,
                theme = app.theme().display_name(),
                bar = app.preferences.bar.id()
            )
        ),
        style::base(),
    )));
    lines.push(Line::from(Span::styled(
        format!("  {}", t!("tui.help.preferences_config")),
        style::muted(),
    )));
    lines.push(Line::from(""));
    lines.push(section_title(t!("tui.help.states")));
    for text in [
        t!("tui.help.states_1"),
        t!("tui.help.states_2"),
        t!("tui.help.states_3"),
        t!("tui.help.states_4"),
    ] {
        lines.push(Line::from(Span::styled(format!("  {text}"), style::base())));
    }
    lines.push(Line::from(""));
    lines.push(section_title(t!("tui.help.notes")));
    let notes = app.notes();
    if notes.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", t!("tui.help.no_notes")),
            style::muted(),
        )));
    }
    for note in notes {
        for piece in wrap(&note, width.saturating_sub(4)) {
            lines.push(Line::from(Span::styled(
                format!("  {piece}"),
                style::muted(),
            )));
        }
    }
    for (n, line) in lines
        .iter()
        .take(area.height.saturating_sub(1) as usize)
        .enumerate()
    {
        frame.render_widget(
            line.clone(),
            Rect::new(area.x, area.y + n as u16, area.width, 1),
        );
    }
    let (back, help, quit) = (
        t!("tui.footer.back"),
        t!("tui.footer.help"),
        t!("tui.footer.quit"),
    );
    frame.render_widget(
        footer(
            &[
                &[("Esc, h", &back), ("?", &help), ("q", &quit)],
                &[("Esc", &back), ("q", &quit)],
            ],
            width,
        ),
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
    );
}

/// A section title in the help.
fn section_title(title: String) -> Line<'static> {
    Line::from(Span::styled(format!(" {title}"), style::accent()))
}

/// The first and one-past-last rows to show of a list of `total`, with
/// `selected` in view: the cursor never leaves the window, and the list only
/// scrolls when the cursor reaches the edge.
pub fn window(total: usize, selected: usize, height: usize) -> (usize, usize) {
    if height == 0 || total <= height {
        return (0, total.min(height));
    }
    let start = selected.saturating_sub(height / 2).min(total - height);
    (start, start + height)
}

/// The widest value the values column has to hold, so the bar does not steal
/// room from what is read.
fn widest_primary(rows: &[Row]) -> usize {
    rows.iter()
        .filter(|row| !row.is_status())
        .map(|row| row.primary.chars().count())
        .max()
        .unwrap_or(20)
}

/// The category name, in the current language.
pub fn category_label(category: Category) -> String {
    match category {
        Category::Search => t!("tui.category.search"),
        Category::Llm => t!("tui.category.llm"),
        Category::Subscription => t!("tui.category.subscription"),
        Category::Other => t!("tui.category.other"),
    }
}

/// The level name, as the traffic light writes it. Level labels are at most
/// 8 characters (the level column is sized for that; the muted color is what
/// marks "no ref." as secondary).
pub fn level_label(level: Level) -> String {
    match level {
        Level::Ok => t!("tui.level.ok"),
        Level::Warning => t!("tui.level.warning"),
        Level::Critical => t!("tui.level.critical"),
        Level::Exhausted => t!("tui.level.exhausted"),
        Level::NoReference => t!("tui.level.no_reference"),
    }
}

/// Cuts the text to the given width, with `…` saying something was left out.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut cut: String = text.chars().take(width - 1).collect();
    cut.push('…');
    cut
}

/// [`truncate`] followed by padding to the exact width, so columns line up.
pub fn pad(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let room = width.saturating_sub(text.chars().count());
    format!("{text}{}", " ".repeat(room))
}

/// Wraps the text into lines of `width`, breaking at spaces.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
        while current.chars().count() > width {
            let piece: String = current.chars().take(width).collect();
            lines.push(piece);
            current = current.chars().skip(width).collect();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// [`wrap`] for the whole text at once (all the notes joined).
fn wrap_all(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    wrap(text, width)
}
