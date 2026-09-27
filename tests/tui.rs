//! Screen tests.
//!
//! The screen is drawn on a `TestBackend` — no terminal — and the text and
//! colors in the resulting `Buffer` are compared. What is proven here: the
//! numbers are the fixture's (none made up), the traffic light has the level's
//! color, the bar is the fraction left, below 60×12 only the message is shown,
//! the whole screen follows the chosen language — and no credential ever
//! appears on any screen.
//!
//! Tests run in English (the default language). The i18n language is per
//! thread; tests that switch it reset it at the end.

mod common;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

use quotop::config::Config;
use quotop::credentials::{self, Source};
use quotop::engine;
use quotop::i18n;
use quotop::model::Cache;
use quotop::tui::app::{App, Sources, View};
use quotop::tui::theme::{BarStyle, CATALOG, Theme};
use quotop::tui::{self, view};

/// The value put in place of the Tavily key: if it shows up on screen, the
/// screen is showing credentials.
const FAKE_SECRET: &str = "FAKE_TAVILY_must_never_appear_0123456789";

/// The golden fixture of 2026-09-26, the same one the `providers` tests use.
fn fixture() -> Cache {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/readings_2026-09-26.json"
    );
    let text = std::fs::read_to_string(path).expect("read the fixture");
    serde_json::from_str(&text).expect("the fixture is a valid `Cache`")
}

/// A fixed instant, three minutes after the fixture: with no clock ticking,
/// what the header says does not change between runs.
fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-26T11:56:04Z")
        .expect("test instant")
        .with_timezone(&Utc)
}

/// The app with the fixture in it and **every** service visible (as if they
/// had all been chosen in the `s` screen), with no refresh at all (no network:
/// the whole list comes from the cache).
fn app() -> App {
    let mut app = app_without_choices();
    let ids: Vec<String> = app
        .sources
        .providers
        .iter()
        .map(|p| p.id().to_string())
        .collect();
    app.visibility.show.extend(ids);
    app
}

/// The app with the fixture and the default visibility: only services with a
/// credential appear (here, only Tavily has one).
fn app_without_choices() -> App {
    let temp = common::Temp::new("tui");
    let config = Config::default();
    let providers = engine::registry(&config);
    let mut env: Vec<(String, String)> = Vec::new();
    for provider in &providers {
        if provider.id() == "tavily" {
            for name in provider.variables() {
                env.push((name.to_string(), FAKE_SECRET.to_string()));
            }
        }
    }
    let source = Source {
        env,
        key_files: Vec::new(),
        home_dir: Some(temp.root().to_path_buf()),
    };
    let cred = credentials::load(&source, &mut Vec::new());
    App::new(
        Sources {
            providers: Arc::new(providers),
            config: Arc::new(config),
            cred: Arc::new(cred),
        },
        Some(fixture()),
        now(),
        Vec::new(),
        None,
    )
}

/// Draws the screen and returns it as text (one string per row) plus the
/// cells, so the colors can be read.
fn screen(app: &App, width: u16, height: u16) -> (Vec<String>, Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| view::draw(frame, app))
        .expect("draw the screen");
    let buffer = terminal.backend().buffer().clone();
    let area = *buffer.area();
    let rows = (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                .collect::<String>()
        })
        .collect();
    (rows, buffer)
}

/// The screen row where `target` appears.
fn row_with<'a>(rows: &'a [String], target: &str) -> Option<&'a String> {
    rows.iter().find(|row| row.contains(target))
}

/// The foreground color of the first cell where `target` starts.
fn color_of(buffer: &Buffer, target: &str) -> Option<Color> {
    let area = *buffer.area();
    let expected: Vec<char> = target.chars().collect();
    for y in area.y..area.y + area.height {
        let row: Vec<char> = (area.x..area.x + area.width)
            .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
            .flat_map(|symbol| symbol.chars().collect::<Vec<char>>())
            .collect();
        if expected.len() > row.len() {
            continue;
        }
        for start in 0..=row.len() - expected.len() {
            if row[start..start + expected.len()] == expected[..] {
                let x = area.x + start as u16;
                return buffer.cell((x, y)).map(|cell| cell.fg);
            }
        }
    }
    None
}

/// The foreground color of the first cell with `symbol` on the row where
/// `target` appears.
fn color_in_row(buffer: &Buffer, target: &str, symbol: char) -> Option<Color> {
    let area = *buffer.area();
    (area.y..area.y + area.height).find_map(|y| {
        let cells: Vec<_> = (area.x..area.x + area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .collect();
        let text: String = cells.iter().map(|cell| cell.symbol()).collect();
        if !text.contains(target) {
            return None;
        }
        cells
            .iter()
            .find(|cell| cell.symbol() == symbol.to_string())
            .map(|cell| cell.fg)
    })
}

/// How many cells with a glyph (not spaces) are painted in `color` on the row
/// where `target` appears.
fn cells_with_color(buffer: &Buffer, target: &str, color: Color) -> usize {
    let area = *buffer.area();
    (area.y..area.y + area.height)
        .find_map(|y| {
            let cells: Vec<_> = (area.x..area.x + area.width)
                .filter_map(|x| buffer.cell((x, y)))
                .collect();
            let text: String = cells.iter().map(|cell| cell.symbol()).collect();
            text.contains(target).then(|| {
                cells
                    .iter()
                    .filter(|cell| cell.fg == color && cell.symbol().trim() != "")
                    .count()
            })
        })
        .unwrap_or(0)
}

/// Does any cell on the screen use this color?
fn color_exists(buffer: &Buffer, color: Color) -> bool {
    let area = *buffer.area();
    (area.y..area.y + area.height).any(|y| {
        (area.x..area.x + area.width).any(|x| {
            buffer
                .cell((x, y))
                .map(|cell| cell.fg == color)
                .unwrap_or(false)
        })
    })
}

#[test]
fn the_list_shows_what_the_fixture_says() {
    let app = app();
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");

    // The header sums up the registry's 26 services by level (1 critical, 1
    // failed, 12 ok, 12 without data) and shows the age of the readings; with
    // room to spare, it also shows the total.
    for chip in ["1 critical", "1 failed", "12 ok", "12 no data"] {
        assert!(rows[0].contains(chip), "missing «{chip}»: {}", rows[0]);
    }
    assert!(rows[0].contains("3m ago"), "{}", rows[0]);
    let (wide, _) = screen(&app, 120, 30);
    assert!(wide[0].contains("26 services"), "{}", wide[0]);
    assert!(wide[0].contains("updated 3m ago"), "{}", wide[0]);

    // The home screen is a single list: no sections (the category only
    // appears in the `s` screen and in the detail).
    assert!(rows[2].contains("26 services"), "{}", rows[2]);
    // ("Search" is not checked: it is part of "Brave Search".)
    for section in ["Subscriptions", "Other"] {
        assert!(
            row_with(&rows, section).is_none(),
            "section {section} is left over"
        );
    }
    // Services with numbers first, then the ones that failed, last the ones
    // without data.
    let position = |name: &str| {
        rows.iter()
            .position(|row| row.contains(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert!(position("ElevenLabs") < position("Exa"), "{text}");
    assert!(position("Exa") < position("DeepSeek"), "{text}");

    // OpenRouter has 0.43 USD out of 30: it is the fixture's CRITICAL case, and
    // the number shown is the one in there (rounded, not made up).
    let openrouter = row_with(&rows, "OpenRouter").expect("OpenRouter row");
    assert!(openrouter.contains("0.43"), "{openrouter}");
    assert!(openrouter.contains("CRITICAL"), "{openrouter}");

    // Brave is paid per request: it gets the `$` and stays out of the automatic
    // refresh.
    let brave = row_with(&rows, "Brave").expect("Brave row");
    assert!(brave.contains("Brave Search $"), "{brave}");
    assert!(brave.contains("1977"), "{brave}");

    // Claude has no documented endpoint: it gets the `†` and shows both
    // meters, one per row.
    assert!(row_with(&rows, "Claude †").is_some(), "{text}");
    let five_hours = row_with(&rows, "69% used").expect("5h meter");
    assert!(five_hours.contains("Claude"), "{five_hours}");
    // The label comes from the provider (English, or still Portuguese in an
    // older fixture) and is shown through `i18n::label`.
    assert!(
        row_with(&rows, "7 days")
            .or_else(|| row_with(&rows, "7 dias"))
            .is_some(),
        "{text}"
    );

    // Without meters, the row is the status — and it says why.
    let exa = row_with(&rows, "Exa").expect("Exa row");
    assert!(exa.contains("no balance API"), "{exa}");
    let deepseek = row_with(&rows, "DeepSeek").expect("DeepSeek row");
    assert!(deepseek.contains("no credential"), "{deepseek}");

    // No status row makes up numbers.
    assert!(
        !deepseek.chars().any(|c| c.is_ascii_digit()),
        "the DeepSeek row must have no numbers: {deepseek}"
    );
}

#[test]
fn the_bar_is_the_fraction_left() {
    let app = app();
    let (rows, buffer) = screen(&app, 80, 30);

    let theme = Theme::default_theme();

    // Tavily has 949 of 1000 credits left: an almost full bar (the default bar
    // is the `━` line, with `╸` closing a half cell). The filled part and the
    // track use the same `━`: what tells them apart is the color.
    let tavily = row_with(&rows, "51 / 1000").expect("Tavily row");
    let filled = cells_with_color(&buffer, "51 / 1000", theme.bar);
    let track = cells_with_color(&buffer, "51 / 1000", theme.track);
    assert!(
        filled >= 7 && track <= 1,
        "Tavily's bar should be full: {tavily}"
    );
    assert_eq!(
        cells_with_color(&buffer, "0.43", theme.error),
        1 + "CRITICAL".chars().count() + 1,
        "on OpenRouter only the minimum mark, the symbol and CRITICAL are red"
    );

    // OpenRouter has 0.43 of 30: the filled part is just the minimum mark, and
    // the rest is track (the same `━`, in the track color).
    let (filled, empty) = BarStyle::Line.render(0.43 / 30.0, 10);
    assert_eq!(filled, "╸", "a small remainder still leaves a mark");
    assert_eq!(empty.chars().count(), 9);
    assert_eq!(
        color_in_row(&buffer, "0.43", '╸'),
        Some(theme.error),
        "OpenRouter's mark is red"
    );

    // With no reference to compare against (Serper), there is no bar: only a
    // light dashed line.
    let serper = row_with(&rows, "Serper").expect("Serper row");
    assert!(serper.contains('╌'), "{serper}");
    assert!(!serper.contains('━'), "{serper}");

    // An `ok` bar uses the theme's bar color (not the traffic light's green),
    // and the track has its own color.
    assert!(
        color_exists(&buffer, theme.bar),
        "the `ok` bar color is missing"
    );
    assert!(
        color_exists(&buffer, theme.track),
        "the track color is missing"
    );
}

#[test]
fn the_traffic_light_has_the_level_color() {
    let app = app();
    let (_, buffer) = screen(&app, 80, 30);

    let theme = Theme::default_theme();
    assert_eq!(color_of(&buffer, "CRITICAL"), Some(theme.error));
    assert_eq!(color_of(&buffer, "✕ CRITICAL"), Some(theme.error));
    assert_eq!(color_of(&buffer, "no ref."), Some(theme.muted));
    assert_eq!(
        color_of(&buffer, "WARNING"),
        None,
        "the fixture has no warnings"
    );
}

#[test]
fn below_the_minimum_only_the_message_is_shown() {
    let app = app();
    // Too narrow and too short, too narrow with height to spare, and the exact
    // minimum width with one row too few.
    for (width, height) in [(50, 10), (50, 20), (60, 11)] {
        let size = format!("{width}×{height}");
        let (rows, _) = screen(&app, width, height);
        let text = rows.join("\n");

        assert!(
            text.contains("terminal too small (min. 60×12)"),
            "{size}: {text}"
        );
        // No data: no list, no header, no secrets.
        assert!(!text.contains("Tavily"), "{size}: {text}");
        assert!(!text.contains("quotop"), "{size}: {text}");
        assert!(!text.contains(FAKE_SECRET), "{size}: {text}");
        let written = rows.iter().filter(|row| !row.trim().is_empty()).count();
        assert_eq!(
            written, 1,
            "{size}: only the message may be written: {rows:?}"
        );
    }
}

#[test]
fn no_credential_appears_on_any_screen() {
    let mut app = app();
    // The value is in there: the app has it among its credentials, and Tavily
    // is one of the visible rows.
    assert!(
        app.sources
            .cred
            .all_exposed()
            .iter()
            .any(|secret| secret.expose() == FAKE_SECRET),
        "the test needs the fake credential inside the app"
    );

    for (width, height) in [(80u16, 30u16), (120, 40), (61, 13)] {
        let (rows, _) = screen(&app, width, height);
        let text = rows.join("\n");
        assert!(
            !text.contains(FAKE_SECRET),
            "credential on the {width}×{height} screen: {text}"
        );
        assert!(
            text.contains("Tavily"),
            "Tavily should be visible at {width}×{height}"
        );
    }

    // Nor do the detail, the help or the overlays.
    for view in [View::Detail, View::Help, View::Preferences, View::Services] {
        app.view = view;
        let (rows, _) = screen(&app, 80, 30);
        let text = rows.join("\n");
        assert!(
            !text.contains(FAKE_SECRET),
            "credential in {view:?}: {text}"
        );
    }
}

#[test]
fn the_filter_hides_services_without_numbers() {
    let mut app = app();
    let (before, _) = screen(&app, 80, 30);
    assert!(row_with(&before, "DeepSeek").is_some());

    // `f` hides the rows without a credential and without a balance API.
    let request = tui::handle_key(&mut app, key('f'));
    assert!(request.is_none(), "`f` asks for no refresh");
    let (after, _) = screen(&app, 80, 30);
    assert!(
        row_with(&after, "Tavily").is_some(),
        "services with numbers stay"
    );
    assert!(
        row_with(&after, "DeepSeek").is_none(),
        "services without numbers go"
    );
    assert!(row_with(&after, "Exa").is_none(), "Exa goes too");

    // And they come back with another press.
    tui::handle_key(&mut app, key('f'));
    let (back, _) = screen(&app, 80, 30);
    assert!(row_with(&back, "DeepSeek").is_some());
}

#[test]
fn the_keys_do_what_they_say() {
    let mut app = app();

    // `j` goes down in registry order (Tavily first, Firecrawl second); the
    // selection starts at the top of the list.
    app.move_by(isize::MIN);
    assert_eq!(
        app.selected_row().expect("initial selection").provider,
        "tavily"
    );
    tui::handle_key(&mut app, key('j'));
    assert_eq!(app.selected_row().expect("after j").provider, "firecrawl");
    tui::handle_key(&mut app, key('k'));
    assert_eq!(app.selected_row().expect("after k").provider, "tavily");
    // `g` and `G` go to the ends without stepping outside the list.
    tui::handle_key(&mut app, key('G'));
    assert_eq!(app.selected_row().expect("after G").provider, "x");
    tui::handle_key(&mut app, key('g'));
    assert_eq!(app.selected_row().expect("after g").provider, "tavily");

    // `Enter` opens the detail and `Esc` goes back.
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    assert_eq!(app.view, View::Detail);
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    assert!(text.contains("Detail · Tavily"), "{text}");
    assert!(text.contains("Endpoint"), "{text}");
    // The endpoint is shown without a query string: the URL, and no probe.
    let endpoint = row_with(&rows, "https://").expect("endpoint row");
    assert!(!endpoint.contains('?'), "{endpoint}");
    assert!(!endpoint.contains("api_key"), "{endpoint}");

    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    assert_eq!(app.view, View::List);

    // `r` asks for the selected row only and `R` for everything, both
    // including the per-request services.
    let only_it = tui::handle_key(&mut app, key('r')).expect("`r` asks for a read");
    assert_eq!(only_it, quotop::engine::Filter::One("tavily".to_string()));
    let everything = tui::handle_key(&mut app, key('R')).expect("`R` asks for a read");
    assert_eq!(everything, quotop::engine::Filter::All);

    // `p` opens the preferences, `s` the services; `Esc` closes both.
    tui::handle_key(&mut app, key('p'));
    assert_eq!(app.view, View::Preferences);
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    tui::handle_key(&mut app, key('s'));
    assert_eq!(app.view, View::Services);
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    assert_eq!(app.view, View::List);

    // `?` opens the help and `q` quits.
    tui::handle_key(&mut app, key('?'));
    assert_eq!(app.view, View::Help);
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    assert!(text.contains("help"), "{text}");
    assert!(
        text.contains("preferences: language, theme and bar style"),
        "{text}"
    );
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    tui::handle_key(&mut app, key('q'));
    assert!(app.quit);
}

#[test]
fn the_terminal_is_never_left_in_raw_mode() {
    // The TUI cannot run here (the test has no terminal): what is checked is
    // that the binary refuses, with code 2, when there is no interactive
    // terminal — the path that avoids starting raw mode at all.
    let home = common::Temp::new("tui-no-terminal");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_quotop"))
        .env("HOME", home.root())
        .env("XDG_CONFIG_HOME", home.root().join("config"))
        .env("XDG_CACHE_HOME", home.root().join("cache"))
        .output()
        .expect("run the binary");
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    // The message is the TUI refusing (and not a configuration error).
    assert!(error.contains("--json"), "{error}");
}

#[test]
fn the_preferences_screen_changes_the_theme_and_only_saves_on_enter() {
    let temp = common::Temp::new("tui-preferences");
    let path = temp.root().join("preferences.toml");
    let mut app = app();
    app.load_preferences(Some(path.clone()));
    let background = |app: &App| {
        let (_, buffer) = screen(app, 80, 30);
        // The bottom-right corner is pure background (the header badge has
        // its own background).
        buffer.cell((79, 29)).map(|cell| cell.bg)
    };
    assert_eq!(background(&app), Some(CATALOG[0].background));

    // `p` opens it, `j` moves to the next theme and the list underneath
    // already shows it.
    tui::handle_key(&mut app, key('p'));
    assert_eq!(app.view, View::Preferences);
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    for theme in CATALOG {
        assert!(
            text.contains(theme.name),
            "theme {} is missing: {text}",
            theme.name
        );
    }
    for language in i18n::LANGUAGES {
        assert!(
            text.contains(language.name),
            "language {} is missing: {text}",
            language.name
        );
    }
    tui::handle_key(&mut app, key('j'));
    assert_eq!(background(&app), Some(CATALOG[1].background));
    tui::handle_key(&mut app, key('b'));
    assert_eq!(app.preferences.bar, BarStyle::Blocks);

    // `Esc` goes back to what was there, without saving anything.
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    assert_eq!(app.view, View::List);
    assert_eq!(background(&app), Some(CATALOG[0].background));
    assert_eq!(app.preferences.bar, BarStyle::Line);
    assert!(!path.exists(), "Esc does not save");

    // `k` on the first theme wraps around to the last; `Enter` saves.
    tui::handle_key(&mut app, key('p'));
    tui::handle_key(&mut app, key('k'));
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    assert_eq!(app.view, View::List);
    let last = CATALOG.len() - 1;
    assert_eq!(app.preferences.theme, last);
    let saved = quotop::tui::theme::read(&path).expect("the choice was saved");
    assert_eq!(saved.theme, last);

    // In a new run, the saved choice is the one that counts.
    let mut other = self::app();
    other.load_preferences(Some(path));
    assert_eq!(other.preferences.theme, last);
}

#[test]
fn switching_to_portuguese_translates_the_whole_screen() {
    let portuguese = i18n::index_of("pt-PT").expect("pt-PT exists");
    let mut app = app();

    // `p` then `l`: the language changes right away, the list underneath
    // included.
    tui::handle_key(&mut app, key('p'));
    tui::handle_key(&mut app, key('l'));
    assert_eq!(app.preferences.language, portuguese);
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    assert!(text.contains("Preferências"), "{text}");
    assert!(text.contains("Idioma"), "{text}");
    assert!(text.contains("sem credencial"), "{text}");

    // `Esc` reverts the language too.
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    assert_eq!(app.preferences.language, i18n::DEFAULT);
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    assert!(text.contains("no credential"), "{text}");
    assert!(!text.contains("sem credencial"), "{text}");

    // Back to Portuguese, and look at the list and the help.
    tui::handle_key(&mut app, key('p'));
    tui::handle_key(&mut app, key('l'));
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    let openrouter = row_with(&rows, "OpenRouter").expect("OpenRouter row");
    assert!(openrouter.contains("0,43"), "decimal comma: {openrouter}");
    assert!(openrouter.contains("CRÍTICO"), "{openrouter}");
    assert!(rows[0].contains("1 crítico"), "{}", rows[0]);
    assert!(rows[0].contains("há 3 min"), "{}", rows[0]);
    assert!(text.contains("26 serviços"), "{text}");
    let deepseek = row_with(&rows, "DeepSeek").expect("DeepSeek row");
    assert!(deepseek.contains("sem credencial"), "{deepseek}");

    tui::handle_key(&mut app, key('?'));
    let (rows, _) = screen(&app, 80, 30);
    assert!(rows[0].contains("ajuda"), "{}", rows[0]);
    let text = rows.join("\n");
    assert!(text.contains("Marcadores e sinais"), "{text}");
    assert!(text.contains("ESGOTADO"), "{text}");

    i18n::set_current(i18n::DEFAULT);
}

#[test]
fn the_saved_language_is_loaded_in_a_new_run() {
    let portuguese = i18n::index_of("pt-PT").expect("pt-PT exists");
    let temp = common::Temp::new("tui-language");
    let path = temp.root().join("state").join("preferences.toml");
    let mut app = app();
    app.load_preferences(Some(path.clone()));

    tui::handle_key(&mut app, key('p'));
    tui::handle_key(&mut app, key('l'));
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    let text = std::fs::read_to_string(&path).expect("preferences saved");
    assert!(text.contains("language = \"pt-PT\""), "{text}");

    // A new app (starting in English) loads the file and is in Portuguese.
    i18n::set_current(i18n::DEFAULT);
    let mut other = self::app();
    assert_eq!(other.preferences.language, i18n::DEFAULT);
    other.load_preferences(Some(path));
    assert_eq!(other.preferences.language, portuguese);
    let (rows, _) = screen(&other, 80, 30);
    let deepseek = row_with(&rows, "DeepSeek").expect("DeepSeek row");
    assert!(deepseek.contains("sem credencial"), "{deepseek}");

    i18n::set_current(i18n::DEFAULT);
}

#[test]
fn every_theme_draws_without_losing_text() {
    let mut app = app();
    for (index, theme) in CATALOG.iter().enumerate() {
        app.preferences.theme = index;
        for bar_style in BarStyle::ALL {
            app.preferences.bar = bar_style;
            let (rows, buffer) = screen(&app, 80, 30);
            let text = rows.join("\n");
            assert!(
                text.contains("OpenRouter"),
                "{} {bar_style:?}: {text}",
                theme.id
            );
            assert_eq!(
                color_of(&buffer, "CRITICAL"),
                Some(theme.error),
                "{} {bar_style:?}",
                theme.id
            );
        }
    }
}

#[test]
fn without_choices_the_screen_shows_only_configured_services() {
    // In the test app only Tavily has a key: it is the only one shown, even
    // though the others have cached readings.
    let app = app_without_choices();
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    assert!(text.contains("Tavily"), "{text}");
    for hidden in ["OpenRouter", "DeepSeek", "Claude", "Exa"] {
        assert!(!text.contains(hidden), "{hidden} has no key: {text}");
    }
    assert!(text.contains("1 service"), "{text}");
    assert!(rows[0].contains("1 ok"), "{}", rows[0]);
}

/// A test app with the key file and the credential source in a temporary
/// directory (nothing touches the real home).
fn app_with_key_file(temp: &common::Temp, env: Vec<(String, String)>) -> App {
    let mut app = app_without_choices();
    let keys_file = temp.root().join("config").join("keys.env");
    let mut full_env = vec![("TAVILY_API_KEY".to_string(), FAKE_SECRET.to_string())];
    full_env.extend(env);
    app.keys_file = Some(keys_file.clone());
    app.services_path = Some(temp.root().join("state").join("services.toml"));
    app.credential_source = Some(Source {
        env: full_env,
        key_files: vec![keys_file],
        home_dir: Some(temp.root().to_path_buf()),
    });
    app
}

/// Puts the menu cursor on the given service.
fn cursor_on(app: &mut App, id: &str) {
    app.menu.cursor = app
        .sources
        .providers
        .iter()
        .position(|p| p.id() == id)
        .expect("the service exists");
}

#[test]
fn the_services_screen_shows_and_hides_services() {
    let temp = common::Temp::new("tui-menu");
    let mut app = app_with_key_file(&temp, Vec::new());

    tui::handle_key(&mut app, key('s'));
    assert_eq!(app.view, View::Services);
    let (rows, _) = screen(&app, 100, 40);
    let text = rows.join("\n");
    // The menu lists every service, with its category and the key state.
    for name in ["Tavily", "DeepSeek", "Twilio", "Claude"] {
        assert!(text.contains(name), "missing {name}: {text}");
    }
    let tavily = row_with(&rows, "[x] Tavily").expect("Tavily checked");
    assert!(tavily.contains("Search"), "{tavily}");
    assert!(tavily.contains("✓ TAVILY_API_KEY"), "{tavily}");
    let deepseek = row_with(&rows, "[ ] DeepSeek").expect("DeepSeek unchecked");
    assert!(deepseek.contains("· DEEPSEEK_API_KEY"), "{deepseek}");

    // Space shows DeepSeek (and saves right away); its cached reading already
    // exists, so no new one is requested.
    cursor_on(&mut app, "deepseek");
    let request = tui::handle_key(&mut app, key(' '));
    assert!(request.is_none(), "{request:?}");
    let saved =
        quotop::tui::visibility::read(app.services_path.as_deref().expect("path")).expect("saved");
    assert!(saved.show.contains("deepseek"));

    // Hide Tavily and close: the list only has DeepSeek.
    cursor_on(&mut app, "tavily");
    tui::handle_key(&mut app, key(' '));
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    assert_eq!(app.view, View::List);
    let (rows, _) = screen(&app, 80, 30);
    let text = rows.join("\n");
    assert!(row_with(&rows, "DeepSeek").is_some(), "{text}");
    assert!(row_with(&rows, "Tavily").is_none(), "{text}");
    assert_eq!(app.selected_row().expect("selection").provider, "deepseek");

    // A service that becomes visible without any reading asks for one (if it
    // is free).
    app.readings.retain(|reading| reading.provider != "jina");
    tui::handle_key(&mut app, key('s'));
    cursor_on(&mut app, "jina");
    let request = tui::handle_key(&mut app, key(' '));
    assert_eq!(
        request,
        Some(quotop::engine::Filter::One("jina".to_string()))
    );
}

#[test]
fn typing_a_key_in_the_menu_saves_it_and_never_shows_it() {
    const KEY: &str = "sk-chimera-q-9f8e7d6c5b4a";
    let temp = common::Temp::new("tui-key");
    let mut app = app_with_key_file(&temp, Vec::new());
    tui::handle_key(&mut app, key('s'));
    cursor_on(&mut app, "deepseek");
    tui::handle_key(&mut app, key('a'));
    assert!(app.menu.entry.is_some());

    // Every key is text: the `q` inside the key does not quit the app.
    for character in KEY.chars() {
        assert!(tui::handle_key(&mut app, key(character)).is_none());
    }
    assert!(!app.quit, "a `q` inside a key must not quit");
    let (rows, _) = screen(&app, 100, 40);
    let text = rows.join("\n");
    assert!(!text.contains(KEY), "the key appeared on screen: {text}");
    assert!(
        !text.contains("chimera"),
        "a piece of the key appeared: {text}"
    );
    assert!(text.contains(&"•".repeat(KEY.len())), "{text}");
    assert!(
        text.contains(&format!("{} characters", KEY.len())),
        "{text}"
    );

    let request = tui::handle_key(&mut app, named_key(KeyCode::Enter));
    assert_eq!(
        request,
        Some(quotop::engine::Filter::One("deepseek".to_string()))
    );
    let file = app.keys_file.clone().expect("file");
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        format!("DEEPSEEK_API_KEY={KEY}\n")
    );
    // The credentials were read again: DeepSeek has a key and now appears.
    let deepseek = app.provider_by_id("deepseek").expect("deepseek");
    assert!(app.has_credential(deepseek));
    assert!(app.is_visible(deepseek));
    let (rows, _) = screen(&app, 100, 40);
    let text = rows.join("\n");
    assert!(text.contains("DEEPSEEK_API_KEY saved"), "{text}");
    assert!(!text.contains(KEY), "{text}");
}

#[test]
fn twilio_asks_for_both_variables_and_claude_for_none() {
    let temp = common::Temp::new("tui-twilio");
    let mut app = app_with_key_file(&temp, Vec::new());
    tui::handle_key(&mut app, key('s'));

    cursor_on(&mut app, "claude");
    tui::handle_key(&mut app, key('a'));
    assert!(app.menu.entry.is_none(), "Claude has no key to type");
    let (message, error) = app.menu.message.clone().expect("message");
    assert!(error && message.contains("Claude Code"), "{message}");

    cursor_on(&mut app, "twilio");
    tui::handle_key(&mut app, key('a'));
    // An empty value is rejected and the entry stays on the same variable.
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    assert_eq!(app.menu.entry.as_ref().map(|e| e.index), Some(0));
    for character in "ACfake1".chars() {
        tui::handle_key(&mut app, key(character));
    }
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    assert_eq!(app.menu.entry.as_ref().map(|e| e.index), Some(1));
    for character in "tokenfake".chars() {
        tui::handle_key(&mut app, key(character));
    }
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    let text = std::fs::read_to_string(app.keys_file.clone().expect("file")).expect("read");
    assert_eq!(
        text,
        "TWILIO_ACCOUNT_SID=ACfake1\nTWILIO_AUTH_TOKEN=tokenfake\n"
    );
}

#[test]
fn a_variable_in_the_environment_wins_and_the_menu_says_so() {
    let temp = common::Temp::new("tui-environment");
    let env = vec![("DEEPSEEK_API_KEY".to_string(), "from-env".to_string())];
    let mut app = app_with_key_file(&temp, env);
    tui::handle_key(&mut app, key('s'));
    cursor_on(&mut app, "deepseek");
    tui::handle_key(&mut app, key('a'));
    for character in "from-file".chars() {
        tui::handle_key(&mut app, key(character));
    }
    tui::handle_key(&mut app, named_key(KeyCode::Enter));
    let (message, error) = app.menu.message.clone().expect("message");
    assert!(error && message.contains("in the environment"), "{message}");

    // `Esc` in an entry gives up without saving anything.
    cursor_on(&mut app, "fal");
    tui::handle_key(&mut app, key('a'));
    tui::handle_key(&mut app, key('x'));
    tui::handle_key(&mut app, named_key(KeyCode::Esc));
    assert!(app.menu.entry.is_none());
    assert_eq!(
        app.view,
        View::Services,
        "the entry's Esc does not close the menu"
    );
    let text = std::fs::read_to_string(app.keys_file.clone().expect("file")).expect("read");
    assert!(!text.contains("FAL_KEY"), "{text}");
}

/// Prints the screens as they come out, in every language, for a visual
/// check. Does not run by default:
/// `cargo test --test tui -- --ignored --nocapture print_the_screens`.
#[test]
#[ignore = "visual check: prints the screens"]
fn print_the_screens() {
    let mut app = app();
    for (index, language) in i18n::LANGUAGES.iter().enumerate() {
        app.preferences.language = index;
        app.view = View::List;
        for (width, height) in [(95u16, 24u16), (80, 30), (60, 12), (50, 10)] {
            let (rows, _) = screen(&app, width, height);
            println!("\n===== {} list {width}×{height} =====", language.code);
            for row in &rows {
                println!("{row}");
            }
        }
        for view in [View::Detail, View::Help, View::Preferences, View::Services] {
            app.view = view;
            for (width, height) in [(80u16, 30u16), (60, 12)] {
                let (rows, _) = screen(&app, width, height);
                println!("\n===== {} {view:?} {width}×{height} =====", language.code);
                for row in &rows {
                    println!("{row}");
                }
            }
        }
    }
    i18n::set_current(i18n::DEFAULT);
}

/// A normal key, pressed.
fn key(character: char) -> KeyEvent {
    named_key(KeyCode::Char(character))
}

/// A key pressed without modifiers (the `kind` a real terminal sends).
fn named_key(code: KeyCode) -> KeyEvent {
    let mut key = KeyEvent::new(code, KeyModifiers::NONE);
    key.kind = KeyEventKind::Press;
    key
}
