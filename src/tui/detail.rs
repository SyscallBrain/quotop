//! The detail panel: what the row does not show — every meter with exact
//! values, reset times in local time, class, cost, endpoint (without query
//! string), dashboard, expected variables, last error, the `†` explained and
//! the request duration.

use chrono::{DateTime, Local, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::i18n;
use crate::model::{Class, Cost, Meter, Status, Thresholds, Unit};
use crate::providers::UNDOCUMENTED;
use crate::t;
use crate::tui::app::{self, App};
use crate::tui::style;
use crate::tui::view::{self, category_label};

/// The panel's maximum width: any wider and the reset lines get too long to
/// read in one go.
const MAX_WIDTH: usize = 94;
/// Width of the field labels (`Last error`, `Último erro`), before the value.
const FIELD_WIDTH: usize = 11;

/// Draws the detail of what the selection points at, in a panel over the
/// list: the list stays underneath, visible around the edges.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    // The panel is never narrower than 40 columns nor wider than `MAX_WIDTH`.
    let width = (area.width as usize).saturating_sub(4).clamp(40, MAX_WIDTH);
    let lines = content(app, width.saturating_sub(2));
    // Two border rows plus one footer row: the panel shrinks to its content,
    // so it does not cover the list needlessly.
    let height = (lines.len() as u16 + 3).clamp(5, area.height);
    let panel = centered(area, width as u16, height);
    frame.render_widget(ratatui::widgets::Clear, panel);
    frame.render_widget(
        ratatui::widgets::Block::bordered()
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(style::accent())
            .style(style::base()),
        panel,
    );
    paint(
        frame,
        Rect::new(
            panel.x + 1,
            panel.y + 1,
            panel.width.saturating_sub(2),
            panel.height.saturating_sub(2),
        ),
        lines,
    );
}

/// The requested panel, centered on the screen.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// The panel's lines (without border or footer).
fn content(app: &App, width: usize) -> Vec<Line<'static>> {
    if app.selected.is_none() {
        return vec![
            Line::from(Span::styled(
                format!(" {}", t!("tui.detail.title")),
                style::header(),
            ))
            .style(style::base()),
            Line::from(""),
            Line::from(Span::styled(
                format!(" {}", t!("tui.detail.nothing_selected")),
                style::base(),
            )),
        ];
    }
    let id = app
        .selected
        .as_ref()
        .map(|(id, _)| id.clone())
        .unwrap_or_default();
    let provider = app.provider_by_id(&id);
    let reading = app.reading_of(&id);

    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {} · ", t!("tui.detail.title")), style::muted()),
        Span::styled(
            provider.map(|p| p.service()).unwrap_or("?").to_string(),
            style::header(),
        ),
    ])];
    lines.push(Line::from(""));

    field(&mut lines, &t!("tui.detail.id"), &id, width);
    if let Some(provider) = provider {
        field(
            &mut lines,
            &t!("tui.detail.category"),
            &category_label(provider.category()),
            width,
        );
    }
    if let Some(reading) = reading {
        field(
            &mut lines,
            &t!("tui.detail.class"),
            &class_text(reading.class),
            width,
        );
        field(
            &mut lines,
            &t!("tui.detail.cost"),
            &cost_text(reading.cost),
            width,
        );
    }
    if let Some(provider) = provider {
        field(
            &mut lines,
            &t!("tui.detail.endpoint"),
            provider.endpoint().unwrap_or("—"),
            width,
        );
        field(
            &mut lines,
            &t!("tui.detail.dashboard"),
            provider.dashboard(),
            width,
        );
        field(
            &mut lines,
            &t!("tui.detail.thresholds"),
            &thresholds_text(app.sources.config.thresholds_for(&id)),
            width,
        );
        field(
            &mut lines,
            &t!("tui.detail.variables"),
            &provider.variables().join(", "),
            width,
        );
    }
    match reading {
        Some(reading) => {
            let status = app::status_text(&reading.status);
            field(&mut lines, &t!("tui.detail.status"), &status, width);
            field(
                &mut lines,
                &t!("tui.detail.last_error"),
                if matches!(reading.status, Status::Ok) {
                    "—"
                } else {
                    status.as_str()
                },
                width,
            );
            field(
                &mut lines,
                &t!("tui.detail.duration"),
                &reading
                    .duration_ms
                    .map(|ms| format!("{ms} ms"))
                    .unwrap_or_else(|| "—".to_string()),
                width,
            );
            field(
                &mut lines,
                &t!("tui.detail.read_at"),
                &t!("tui.detail.local_time", time = local_time(reading.read_at)),
                width,
            );
        }
        None => field(
            &mut lines,
            &t!("tui.detail.status"),
            &t!("tui.status.never_read"),
            width,
        ),
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(" {}", t!("tui.detail.meters")),
        style::accent(),
    )));
    match reading {
        Some(reading) if !reading.meters.is_empty() => {
            for meter in &reading.meters {
                let text = meter_detail(meter);
                for (n, piece) in view::wrap(&text, width.saturating_sub(16))
                    .into_iter()
                    .enumerate()
                {
                    let prefix = if n == 0 {
                        format!("   {} ", view::pad(&i18n::label(&meter.label), 12))
                    } else {
                        " ".repeat(16)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(style::level_color(meter.level))),
                        Span::styled(piece, style::base()),
                    ]));
                }
                if let Some(fraction) = app::remaining_fraction(meter) {
                    let bar_width = width.saturating_sub(24).min(32);
                    let (filled, empty) = app.preferences.bar.render(fraction, bar_width);
                    lines.push(Line::from(vec![
                        Span::raw(" ".repeat(16)),
                        Span::styled(filled, Style::default().fg(style::bar_color(meter.level))),
                        Span::styled(empty, Style::default().fg(style::theme().track)),
                        Span::styled(
                            format!(
                                " {}",
                                t!(
                                    "tui.detail.bar_left",
                                    percent = app::number(fraction * 100.0, 1)
                                )
                            ),
                            style::muted(),
                        ),
                    ]));
                }
            }
        }
        _ => lines.push(Line::from(Span::styled(
            format!("   {}", t!("tui.detail.no_meters")),
            style::muted(),
        ))),
    }

    if UNDOCUMENTED.contains(&id.as_str()) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" {}", t!("tui.detail.undocumented")),
            style::muted(),
        )));
    }
    // A useful hint when all that is missing is the key: the screen says what
    // to do, without showing any values.
    if let Some(provider) = provider
        && reading.is_some_and(|reading| matches!(reading.status, Status::NoCredential))
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                " {}",
                t!(
                    "tui.detail.no_credential_hint",
                    variables = provider.variables().join(", ")
                )
            ),
            style::muted(),
        )));
    }
    lines
}

/// Draws the lines from the top, and the footer always on the last row.
fn paint(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let limit = area.height.saturating_sub(1) as usize;
    for (n, line) in lines.iter().take(limit).enumerate() {
        frame.render_widget(
            line.clone(),
            Rect::new(area.x, area.y + n as u16, area.width, 1),
        );
    }
    let (back, read_this_row, read, help, quit) = (
        t!("tui.footer.back"),
        t!("tui.footer.read_this_row"),
        t!("tui.footer.read"),
        t!("tui.footer.help"),
        t!("tui.footer.quit"),
    );
    frame.render_widget(
        view::footer(
            &[
                &[
                    ("Esc, h", &back),
                    ("r", &read_this_row),
                    ("?", &help),
                    ("q", &quit),
                ],
                &[("Esc", &back), ("r", &read), ("?", &help), ("q", &quit)],
                &[("Esc", ""), ("r", ""), ("?", ""), ("q", "")],
            ],
            area.width as usize,
        ),
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
    );
}

/// A panel field: label and value, with the value wrapping onto the next line
/// when it does not fit.
fn field(lines: &mut Vec<Line<'static>>, label: &str, value: &str, width: usize) {
    let value = if value.trim().is_empty() {
        "—"
    } else {
        value
    };
    let indent = FIELD_WIDTH + 3;
    let room = width.saturating_sub(indent);
    for (n, piece) in view::wrap(value, room).into_iter().enumerate() {
        let prefix = if n == 0 {
            format!(" {}  ", view::pad(label, FIELD_WIDTH))
        } else {
            " ".repeat(indent)
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, style::muted()),
            Span::styled(piece, style::base()),
        ]));
    }
}

/// A meter with all the exact numbers and the reset time in local time.
fn meter_detail(meter: &Meter) -> String {
    let mut parts = Vec::new();
    let unit = unit_symbol(meter);
    if let Some(used) = meter.used {
        parts.push(t!("tui.detail.used", value = app::exact(used), unit = unit));
    }
    if let Some(limit) = meter.limit {
        parts.push(t!(
            "tui.detail.limit",
            value = app::exact(limit),
            unit = unit
        ));
    }
    if let Some(remaining) = meter.remaining {
        parts.push(t!(
            "tui.detail.remaining",
            value = app::exact(remaining),
            unit = unit
        ));
    }
    parts.push(match meter.resets_at {
        Some(when) => t!("tui.detail.reset_at", time = local_time(when)),
        None => t!("tui.detail.no_reset"),
    });
    parts.push(match app::remaining_fraction(meter) {
        Some(fraction) => t!(
            "tui.detail.fraction_left",
            level = view::level_label(meter.level),
            percent = app::number(fraction * 100.0, 1)
        ),
        None => t!(
            "tui.detail.no_limit",
            level = view::level_label(meter.level)
        ),
    });
    parts.join(" · ")
}

/// The unit as the detail writes it.
fn unit_symbol(meter: &Meter) -> String {
    match meter.unit {
        Unit::Currency => meter.currency.clone().unwrap_or_default(),
        Unit::Percent => "%".to_string(),
        other => app::unit_name(other),
    }
}

/// A time in local time (the meter keeps everything in UTC).
fn local_time(when: DateTime<Utc>) -> String {
    when.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// The class, in the current language.
fn class_text(class: Class) -> String {
    match class {
        Class::ExactBalance => t!("tui.detail.class_exact_balance"),
        Class::UsageVsLimit => t!("tui.detail.class_usage_vs_limit"),
        Class::RateLimitOnly => t!("tui.detail.class_rate_limit_only"),
        Class::NoApi => t!("tui.detail.class_no_api"),
    }
}

/// The cost, in the current language (`$` is what the automatic refresh never
/// reads).
fn cost_text(cost: Cost) -> String {
    match cost {
        Cost::Free => t!("tui.detail.cost_free"),
        Cost::PerRequest => t!("tui.detail.cost_per_request"),
    }
}

/// The thresholds in effect for this service.
fn thresholds_text(thresholds: Thresholds) -> String {
    match thresholds {
        Thresholds::Fraction { warning, critical } => t!(
            "tui.detail.thresholds_fraction",
            warning = app::exact(warning),
            critical = app::exact(critical)
        ),
        Thresholds::Absolute { warning, critical } => t!(
            "tui.detail.thresholds_absolute",
            warning = app::exact(warning),
            critical = app::exact(critical)
        ),
    }
}
