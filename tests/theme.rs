//! Theme tests: the catalog, the `config.toml` colors, the precedence of the
//! saved preferences and how the bars are drawn.

mod common;

use std::collections::BTreeMap;

use ratatui::style::Color;

use quotop::config;
use quotop::i18n;
use quotop::tui::theme::{self, BarStyle, CATALOG, Preferences, Theme};

#[test]
fn the_catalog_has_unique_ids_and_the_first_is_the_default() {
    let mut ids: Vec<&str> = CATALOG.iter().map(|t| t.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), CATALOG.len(), "duplicated ids in the catalog");
    assert_eq!(Theme::default_theme().id, "tokyo-night");
    assert_eq!(Preferences::default().theme(), Theme::default_theme());
    assert_eq!(Preferences::default().bar, BarStyle::Line);
    assert_eq!(Preferences::default().language, i18n::DEFAULT);
}

#[test]
fn an_ok_bar_is_not_green() {
    // The complaint that started the themes: the `ok` bar has its own color,
    // not the traffic light's green, in every theme.
    for theme in CATALOG {
        assert_ne!(
            theme.bar, theme.ok,
            "{}: the `ok` bar has the traffic-light color",
            theme.id
        );
        assert_ne!(
            theme.bar, theme.track,
            "{}: bar and track are equal",
            theme.id
        );
    }
}

#[test]
fn hand_written_colors() {
    assert_eq!(theme::color("#ff8000"), Some(Color::Rgb(0xff, 0x80, 0x00)));
    assert_eq!(theme::color("FF8000"), Some(Color::Rgb(0xff, 0x80, 0x00)));
    assert_eq!(theme::color(" reset "), Some(Color::Reset));
    for wrong in ["", "#fff", "#gggggg", "red", "#ff80001"] {
        assert_eq!(theme::color(wrong), None, "«{wrong}» is not a color");
    }
}

#[test]
fn config_colors_replace_only_what_they_name() {
    let mut colors = BTreeMap::new();
    colors.insert("bar".to_string(), "#112233".to_string());
    colors.insert("background".to_string(), "reset".to_string());
    colors.insert("error".to_string(), "crimson".to_string());
    colors.insert("glow".to_string(), "#ffffff".to_string());
    let mut warnings = Vec::new();
    let overrides = theme::overrides(&colors, &mut warnings);

    let theme = Theme::default_theme().with_overrides(&overrides);
    assert_eq!(theme.bar, Color::Rgb(0x11, 0x22, 0x33));
    assert_eq!(theme.background, Color::Reset);
    assert_eq!(
        theme.error,
        Theme::default_theme().error,
        "an invalid color is not applied"
    );
    assert_eq!(
        theme.text,
        Theme::default_theme().text,
        "what was not mentioned stays"
    );

    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings.iter().any(|w| w.contains("colors.error")),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("colors.glow")),
        "{warnings:?}"
    );
}

#[test]
fn the_saved_choice_beats_config_and_config_beats_the_default() {
    let mut warnings = Vec::new();
    let nord = Theme::index("nord").expect("nord exists");
    let dracula = Theme::index("dracula").expect("dracula exists");

    assert_eq!(
        theme::resolve(None, None, None, &mut warnings),
        Preferences::default()
    );

    let from_config = theme::resolve(Some("nord"), Some("dots"), None, &mut warnings);
    assert_eq!(
        from_config,
        Preferences {
            language: i18n::DEFAULT,
            theme: nord,
            bar: BarStyle::Dots
        }
    );

    let saved = Preferences {
        language: i18n::index_of("pt-PT").expect("pt-PT exists"),
        theme: dracula,
        bar: BarStyle::Blocks,
    };
    assert_eq!(
        theme::resolve(Some("nord"), Some("dots"), Some(saved), &mut warnings),
        saved
    );
    assert!(warnings.is_empty(), "{warnings:?}");

    // A wrong name is a warning, and the default stays.
    let wrong = theme::resolve(Some("solarized"), Some("dashed"), None, &mut warnings);
    assert_eq!(wrong, Preferences::default());
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0].contains("solarized") && warnings[0].contains("tokyo-night"),
        "{warnings:?}"
    );
    assert!(
        warnings[1].contains("dashed") && warnings[1].contains("line, blocks, dots"),
        "{warnings:?}"
    );
}

#[test]
fn save_and_read_the_preferences() {
    let temp = common::Temp::new("theme-save");
    // The directory does not exist yet: saving creates it.
    let path = temp
        .root()
        .join("state")
        .join("quotop")
        .join(theme::FILE_NAME);
    assert_eq!(theme::read(&path), None, "no file, no choice");

    let choice = Preferences {
        language: i18n::index_of("pt-PT").expect("pt-PT exists"),
        theme: Theme::index("gruvbox").expect("gruvbox exists"),
        bar: BarStyle::Dots,
    };
    theme::save(&path, &choice).expect("save");
    assert_eq!(theme::read(&path), Some(choice));
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("theme = \"gruvbox\""), "{text}");
    assert!(text.contains("language = \"pt-PT\""), "{text}");
    assert!(
        !path.with_extension("toml.tmp").exists(),
        "the temporary file was left behind"
    );

    // Without a `language` key, the current language stays.
    std::fs::write(&path, "theme = \"nord\"\nbar = \"blocks\"\n").expect("write");
    let without_language = theme::read(&path).expect("still a choice");
    assert_eq!(without_language.language, i18n::current_index());
    assert_eq!(without_language.bar, BarStyle::Blocks);

    // A theme that no longer exists is no choice at all.
    std::fs::write(&path, "theme = \"vanished\"\n").expect("write");
    assert_eq!(theme::read(&path), None);
}

#[test]
fn bars_always_have_the_requested_width() {
    for bar_style in BarStyle::ALL {
        for width in [1usize, 6, 10, 17] {
            for step in 0..=40 {
                let fraction = f64::from(step) / 40.0;
                let (filled, empty) = bar_style.render(fraction, width);
                assert_eq!(
                    filled.chars().count() + empty.chars().count(),
                    width,
                    "{bar_style:?} {fraction} in {width}: «{filled}» + «{empty}»"
                );
            }
        }
        // The edges: empty is all track, full has no track, and a tiny
        // remainder still leaves a mark.
        assert_eq!(bar_style.render(0.0, 10).0, "", "{bar_style:?}");
        assert_eq!(bar_style.render(1.0, 10).1, "", "{bar_style:?}");
        assert_eq!(
            bar_style.render(0.0001, 10).0.chars().count(),
            1,
            "{bar_style:?}"
        );
    }
    assert_eq!(
        BarStyle::Line.render(0.55, 10),
        ("━━━━━╸".into(), "━━━━".into())
    );
    assert_eq!(BarStyle::Blocks.render(0.5, 4), ("██".into(), "░░".into()));
}

#[test]
fn config_accepts_theme_bar_and_colors_without_warnings() {
    let temp = common::Temp::new("config-theme");
    let path = temp.file(
        "config.toml",
        "theme = \"nord\"\nbar = \"blocks\"\n\n[colors]\nbar = \"#88c0d0\"\n",
        0o600,
    );
    let loaded = config::load_from(&path).expect("valid config");
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.config.theme.as_deref(), Some("nord"));
    assert_eq!(loaded.config.bar.as_deref(), Some("blocks"));
    assert_eq!(
        loaded.config.colors.get("bar").map(String::as_str),
        Some("#88c0d0")
    );
}
