//! Generates `docs/screenshot.svg`, the screenshot in the README, from the real
//! screen drawn with the recorded readings in `tests/fixtures/`.
//!
//! It is not part of the normal test run. To regenerate the image:
//!
//! ```sh
//! cargo test --test screenshot -- --ignored
//! ```

mod common;

use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use quotop::config::Config;
use quotop::credentials::{self, Source};
use quotop::engine;
use quotop::model::{Cache, Status};
use quotop::tui::app::{App, Sources};
use quotop::tui::view;

const WIDTH: u16 = 100;
const HEIGHT: u16 = 24;
/// Size of one terminal cell in the SVG, in pixels.
const CELL_W: f64 = 8.6;
const CELL_H: f64 = 19.0;
const FONT_SIZE: f64 = 14.0;
/// Room around the terminal for the window frame.
const PAD: f64 = 16.0;
const TITLE_BAR: f64 = 30.0;

#[test]
#[ignore = "generates docs/screenshot.svg"]
fn generate_the_readme_screenshot() {
    let buffer = draw();
    let svg = to_svg(&buffer);
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/screenshot.svg");
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/docs")).expect("docs dir");
    std::fs::write(path, svg).expect("write the screenshot");
    println!("wrote {path}");
}

/// The main screen as a user with keys for these services would see it: the
/// services that have numbers, plus one failing reading.
fn draw() -> Buffer {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/readings_2026-09-26.json"
    );
    let cache: Cache =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("cache");
    let shown: Vec<String> = cache
        .readings
        .iter()
        .filter(|r| !r.meters.is_empty() || matches!(r.status, Status::InvalidCredential { .. }))
        .map(|r| r.provider.clone())
        .collect();

    let temp = common::Temp::new("screenshot");
    let config = Config::default();
    let providers = engine::registry(&config);
    let source = Source {
        env: Vec::new(),
        key_files: Vec::new(),
        home_dir: Some(temp.root().to_path_buf()),
    };
    let cred = credentials::load(&source, &mut Vec::new());
    let now: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-09-26T11:56:04Z")
        .expect("instant")
        .with_timezone(&Utc);
    let mut app = App::new(
        Sources {
            providers: Arc::new(providers),
            config: Arc::new(config),
            cred: Arc::new(cred),
        },
        Some(cache),
        now,
        Vec::new(),
        None,
    );
    app.visibility.show.extend(shown);
    app.move_by(isize::MIN);

    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("terminal");
    terminal
        .draw(|frame| view::draw(frame, &app))
        .expect("draw");
    terminal.backend().buffer().clone()
}

fn hex(color: Color, fallback: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => fallback.to_string(),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn to_svg(buffer: &Buffer) -> String {
    let area = *buffer.area();
    let background = hex(
        buffer
            .cell((area.width - 1, area.height - 1))
            .expect("cell")
            .bg,
        "#1a1b26",
    );
    let term_w = f64::from(area.width) * CELL_W;
    let term_h = f64::from(area.height) * CELL_H;
    let total_w = term_w + 2.0 * PAD;
    let total_h = term_h + TITLE_BAR + PAD;

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {total_w:.0} {total_h:.0}" width="{total_w:.0}" height="{total_h:.0}" font-family="'JetBrains Mono','DejaVu Sans Mono',Menlo,Consolas,monospace" font-size="{FONT_SIZE}">"#
    );
    let _ = writeln!(
        svg,
        r#"<rect width="100%" height="100%" rx="10" fill="{background}"/>"#
    );
    for (i, color) in ["#f7768e", "#e0af68", "#9ece6a"].iter().enumerate() {
        let _ = writeln!(
            svg,
            r#"<circle cx="{}" cy="15" r="6" fill="{color}"/>"#,
            20 + i * 20
        );
    }
    let _ = writeln!(
        svg,
        r##"<text x="{:.0}" y="20" fill="#737aa2" text-anchor="middle" font-size="12">quotop</text>"##,
        total_w / 2.0
    );
    let _ = writeln!(svg, r#"<g transform="translate({PAD} {TITLE_BAR})">"#);

    for y in 0..area.height {
        // Backgrounds first, merged into runs.
        let mut x = 0;
        while x < area.width {
            let cell = buffer.cell((x, y)).expect("cell");
            let bg = hex(cell.bg, &background);
            let start = x;
            while x < area.width && hex(buffer.cell((x, y)).expect("cell").bg, &background) == bg {
                x += 1;
            }
            if bg != background {
                let _ = writeln!(
                    svg,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{CELL_H}" fill="{bg}"/>"#,
                    f64::from(start) * CELL_W,
                    f64::from(y) * CELL_H,
                    f64::from(x - start) * CELL_W
                );
            }
        }
        // Then text, one run per style, every glyph on its own grid column.
        let mut x = 0;
        while x < area.width {
            let cell = buffer.cell((x, y)).expect("cell");
            let style = (
                hex(cell.fg, "#c0caf5"),
                cell.modifier.contains(Modifier::BOLD),
            );
            let mut glyphs = Vec::new();
            while x < area.width {
                let next = buffer.cell((x, y)).expect("cell");
                if (
                    hex(next.fg, "#c0caf5"),
                    next.modifier.contains(Modifier::BOLD),
                ) != style
                {
                    break;
                }
                if next.symbol() != " " {
                    glyphs.push((x, next.symbol().to_string()));
                }
                x += 1;
            }
            if glyphs.is_empty() {
                continue;
            }
            let xs: Vec<String> = glyphs
                .iter()
                .map(|(gx, _)| format!("{:.1}", f64::from(*gx) * CELL_W))
                .collect();
            let text: String = glyphs.iter().map(|(_, g)| escape(g)).collect();
            let weight = if style.1 {
                r#" font-weight="bold""#
            } else {
                ""
            };
            let _ = writeln!(
                svg,
                r#"<text x="{}" y="{:.1}" fill="{}"{weight} xml:space="preserve">{text}</text>"#,
                xs.join(" "),
                f64::from(y) * CELL_H + CELL_H * 0.75,
                style.0
            );
        }
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}
