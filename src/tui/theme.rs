//! Themes, bar styles and the saved preferences: the catalog, the choice and
//! where it is stored.
//!
//! Preferences (language, theme and bar style) come from three sources, in
//! this order of precedence:
//!
//! 1. the choice made in the **Preferences** screen (`p`), saved in
//!    `~/.local/state/quotop/preferences.toml` — the app writes this file, and
//!    only this one (`config.toml` always belongs to the user);
//! 2. `language`, `theme` and `bar` in `config.toml`;
//! 3. the defaults: English, the Tokyo Night theme and the line bar.
//!
//! On top of any theme, the colors of the `[colors]` table in `config.toml`
//! are applied one by one, as `#rrggbb` (or `reset`, the terminal's color).
//!
//! The palettes are the ones each project publishes (`folke/tokyonight.nvim`,
//! `catppuccin/palette`, `morhetz/gruvbox`, `nordtheme`, `dracula-theme`,
//! `rose-pine/palette`). The bar of an `ok` meter uses the theme's bar color,
//! not green: the traffic light only colors what deserves attention.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ratatui::style::Color;

use crate::i18n;

/// Name of the file where the Preferences screen saves the choice.
pub const FILE_NAME: &str = "preferences.toml";

/// The color roles a theme fills in (and that `[colors]` can override).
pub const ROLES: [&str; 11] = [
    "background",
    "text",
    "muted",
    "border",
    "accent",
    "selection",
    "ok",
    "warning",
    "error",
    "bar",
    "track",
];

/// A complete screen palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The name written in `config.toml`.
    pub id: &'static str,
    /// The name the screen shows (see [`Theme::display_name`]).
    pub name: &'static str,
    /// Screen background (solid, so the screen does not inherit the terminal's
    /// — except in the `terminal` theme, which is exactly that).
    pub background: Color,
    /// Normal text.
    pub text: Color,
    /// Secondary text: notes, footer, rows without data.
    pub muted: Color,
    /// Borders and separators.
    pub border: Color,
    /// Titles, key hints and the selected row.
    pub accent: Color,
    /// Background of the selected row.
    pub selection: Color,
    /// The `ok` level.
    pub ok: Color,
    /// The warning level.
    pub warning: Color,
    /// Critical, depleted and errors.
    pub error: Color,
    /// Filled part of an `ok` bar.
    pub bar: Color,
    /// Unfilled part of a bar.
    pub track: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The themes, in the order the Preferences screen lists them. The first one
/// is the default.
pub const CATALOG: [Theme; 8] = [
    Theme {
        id: "tokyo-night",
        name: "Tokyo Night",
        background: rgb(0x1a1b26),
        text: rgb(0xc0caf5),
        muted: rgb(0x737aa2),
        border: rgb(0x3b4261),
        accent: rgb(0x7dcfff),
        selection: rgb(0x292e42),
        ok: rgb(0x9ece6a),
        warning: rgb(0xe0af68),
        error: rgb(0xf7768e),
        bar: rgb(0x7aa2f7),
        track: rgb(0x2f3549),
    },
    Theme {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        background: rgb(0x1e1e2e),
        text: rgb(0xcdd6f4),
        muted: rgb(0x7f849c),
        border: rgb(0x45475a),
        accent: rgb(0xb4befe),
        selection: rgb(0x313244),
        ok: rgb(0xa6e3a1),
        warning: rgb(0xf9e2af),
        error: rgb(0xf38ba8),
        bar: rgb(0x89b4fa),
        track: rgb(0x313244),
    },
    Theme {
        id: "catppuccin-latte",
        name: "Catppuccin Latte",
        background: rgb(0xeff1f5),
        text: rgb(0x4c4f69),
        muted: rgb(0x8c8fa1),
        border: rgb(0xbcc0cc),
        accent: rgb(0x7287fd),
        selection: rgb(0xdce0e8),
        ok: rgb(0x40a02b),
        warning: rgb(0xdf8e1d),
        error: rgb(0xd20f39),
        bar: rgb(0x1e66f5),
        track: rgb(0xccd0da),
    },
    Theme {
        id: "gruvbox",
        name: "Gruvbox",
        background: rgb(0x282828),
        text: rgb(0xebdbb2),
        muted: rgb(0x928374),
        border: rgb(0x504945),
        accent: rgb(0xfabd2f),
        selection: rgb(0x3c3836),
        ok: rgb(0xb8bb26),
        warning: rgb(0xfe8019),
        error: rgb(0xfb4934),
        bar: rgb(0x83a598),
        track: rgb(0x3c3836),
    },
    Theme {
        id: "nord",
        name: "Nord",
        background: rgb(0x2e3440),
        text: rgb(0xd8dee9),
        muted: rgb(0x7b88a1),
        border: rgb(0x4c566a),
        accent: rgb(0x88c0d0),
        selection: rgb(0x3b4252),
        ok: rgb(0xa3be8c),
        warning: rgb(0xebcb8b),
        error: rgb(0xbf616a),
        bar: rgb(0x81a1c1),
        track: rgb(0x3b4252),
    },
    Theme {
        id: "dracula",
        name: "Dracula",
        background: rgb(0x282a36),
        text: rgb(0xf8f8f2),
        muted: rgb(0x6272a4),
        border: rgb(0x44475a),
        accent: rgb(0xff79c6),
        selection: rgb(0x44475a),
        ok: rgb(0x50fa7b),
        warning: rgb(0xf1fa8c),
        error: rgb(0xff5555),
        bar: rgb(0xbd93f9),
        track: rgb(0x383a4a),
    },
    Theme {
        id: "rose-pine",
        name: "Rosé Pine",
        background: rgb(0x191724),
        text: rgb(0xe0def4),
        muted: rgb(0x908caa),
        border: rgb(0x403d52),
        accent: rgb(0xebbcba),
        selection: rgb(0x26233a),
        ok: rgb(0x9ccfd8),
        warning: rgb(0xf6c177),
        error: rgb(0xeb6f92),
        bar: rgb(0xc4a7e7),
        track: rgb(0x26233a),
    },
    Theme {
        id: "terminal",
        name: "Terminal",
        background: Color::Reset,
        text: Color::Reset,
        muted: Color::Indexed(245),
        border: Color::Indexed(240),
        accent: Color::Cyan,
        selection: Color::Indexed(236),
        ok: Color::Green,
        warning: Color::Yellow,
        error: Color::Red,
        bar: Color::Blue,
        track: Color::Indexed(238),
    },
];

impl Theme {
    /// The default theme.
    pub const fn default_theme() -> Theme {
        CATALOG[0]
    }

    /// The index of a theme in the catalog, by id.
    pub fn index(id: &str) -> Option<usize> {
        CATALOG.iter().position(|theme| theme.id == id)
    }

    /// The name as the screen shows it, in the current language: a light
    /// theme and the terminal's own colors say so next to the name.
    pub fn display_name(&self) -> String {
        match self.id {
            "catppuccin-latte" => format!("{} ({})", self.name, crate::t!("tui.preferences.light")),
            "terminal" => format!(
                "{} ({})",
                self.name,
                crate::t!("tui.preferences.terminal_colors")
            ),
            _ => self.name.to_string(),
        }
    }

    /// The theme with the `[colors]` overrides on top.
    pub fn with_overrides(mut self, colors: &ColorOverrides) -> Theme {
        for (role, color) in &colors.0 {
            if let Some(slot) = self.role_mut(role) {
                *slot = *color;
            }
        }
        self
    }

    fn role_mut(&mut self, role: &str) -> Option<&mut Color> {
        Some(match role {
            "background" => &mut self.background,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "border" => &mut self.border,
            "accent" => &mut self.accent,
            "selection" => &mut self.selection,
            "ok" => &mut self.ok,
            "warning" => &mut self.warning,
            "error" => &mut self.error,
            "bar" => &mut self.bar,
            "track" => &mut self.track,
            _ => return None,
        })
    }
}

/// How the bar of the remaining fraction is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarStyle {
    /// `━━━━━╺━━━`: a thin line, with half-cell resolution.
    #[default]
    Line,
    /// `████▌░░░`: solid blocks, with eighth-cell resolution.
    Blocks,
    /// `⣿⣿⣿⡇⣀⣀`: braille, with half-cell resolution.
    Dots,
}

impl BarStyle {
    /// All of them, in the order the `b` key cycles through them.
    pub const ALL: [BarStyle; 3] = [BarStyle::Line, BarStyle::Blocks, BarStyle::Dots];

    /// The name written in `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            BarStyle::Line => "line",
            BarStyle::Blocks => "blocks",
            BarStyle::Dots => "dots",
        }
    }

    /// The style with this `config.toml` name.
    pub fn from_id(id: &str) -> Option<BarStyle> {
        BarStyle::ALL.into_iter().find(|style| style.id() == id)
    }

    /// The next style (the `b` key).
    pub fn next(self) -> BarStyle {
        let n = BarStyle::ALL
            .iter()
            .position(|style| *style == self)
            .unwrap_or(0);
        BarStyle::ALL[(n + 1) % BarStyle::ALL.len()]
    }

    /// The bar of `fraction` (0..=1) in `width` cells: the filled part and the
    /// unfilled part, so they can be painted in different colors.
    ///
    /// A remainder smaller than the resolution must not look like zero: the
    /// thinnest mark says there is still some room left.
    pub fn render(self, fraction: f64, width: usize) -> (String, String) {
        let fraction = fraction.clamp(0.0, 1.0);
        let (full_cell, partial_cells, empty_cell): (char, &[char], char) = match self {
            BarStyle::Line => ('━', &['╸'], '━'),
            BarStyle::Dots => ('⣿', &['⡇'], '⣀'),
            BarStyle::Blocks => ('█', &['▏', '▎', '▍', '▌', '▋', '▊', '▉'], '░'),
        };
        let resolution = partial_cells.len() + 1;
        let mut steps = (fraction * (width * resolution) as f64).floor() as usize;
        if steps == 0 && fraction > 0.0 {
            steps = 1;
        }
        let whole = (steps / resolution).min(width);
        let rest = steps % resolution;
        let mut filled: String = std::iter::repeat_n(full_cell, whole).collect();
        if rest > 0 && whole < width {
            filled.push(partial_cells[rest - 1]);
        }
        let used = filled.chars().count();
        let empty = std::iter::repeat_n(empty_cell, width.saturating_sub(used)).collect();
        (filled, empty)
    }
}

/// The `[colors]` overrides, already validated (role → color).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColorOverrides(pub Vec<(String, Color)>);

/// The chosen preferences: the interface language (index in
/// [`i18n::LANGUAGES`]), the theme (index in [`CATALOG`]) and the bar style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Preferences {
    pub language: usize,
    pub theme: usize,
    pub bar: BarStyle,
}

impl Preferences {
    /// The chosen theme.
    pub fn theme(&self) -> Theme {
        CATALOG[self.theme.min(CATALOG.len() - 1)]
    }

    /// The chosen language.
    pub fn language(&self) -> &'static i18n::Language {
        &i18n::LANGUAGES[self.language.min(i18n::LANGUAGES.len() - 1)]
    }

    /// Switches to the next language (the `l` key), wrapping around.
    pub fn next_language(&mut self) {
        self.language = (self.language + 1) % i18n::LANGUAGES.len();
    }

    /// The text [`save`] writes.
    pub fn to_toml(&self) -> String {
        format!(
            "# Written by the quotop Preferences screen (key `p`). Safe to delete.\n\
             language = \"{}\"\ntheme = \"{}\"\nbar = \"{}\"\n",
            self.language().code,
            self.theme().id,
            self.bar.id()
        )
    }
}

/// The theme and bar to use: the saved ones, else the ones in `config.toml`,
/// else the defaults. An unknown name in `config.toml` is a warning, never an
/// error. The language is resolved elsewhere ([`i18n::resolve`]); the result
/// keeps the saved language, or the current one when nothing was saved.
pub fn resolve(
    config_theme: Option<&str>,
    config_bar: Option<&str>,
    saved: Option<Preferences>,
    warnings: &mut Vec<String>,
) -> Preferences {
    let mut preferences = Preferences {
        language: i18n::current_index(),
        ..Preferences::default()
    };
    if let Some(id) = config_theme {
        match Theme::index(id) {
            Some(index) => preferences.theme = index,
            None => warnings.push(crate::t!(
                "tui.config.unknown_theme",
                id = id,
                themes = CATALOG.iter().map(|t| t.id).collect::<Vec<_>>().join(", ")
            )),
        }
    }
    if let Some(id) = config_bar {
        match BarStyle::from_id(id) {
            Some(style) => preferences.bar = style,
            None => warnings.push(crate::t!(
                "tui.config.unknown_bar",
                id = id,
                bars = BarStyle::ALL.map(BarStyle::id).join(", ")
            )),
        }
    }
    saved.unwrap_or(preferences)
}

/// Validates the `[colors]` table: each known role with a readable color is
/// kept, anything else is a warning.
pub fn overrides(colors: &BTreeMap<String, String>, warnings: &mut Vec<String>) -> ColorOverrides {
    let mut valid = Vec::new();
    for (role, value) in colors {
        if !ROLES.contains(&role.as_str()) {
            warnings.push(crate::t!(
                "tui.config.unknown_color",
                role = role,
                roles = ROLES.join(", ")
            ));
            continue;
        }
        match color(value) {
            Some(parsed) => valid.push((role.clone(), parsed)),
            None => warnings.push(crate::t!(
                "tui.config.bad_color",
                role = role,
                value = value
            )),
        }
    }
    ColorOverrides(valid)
}

/// A hand-written color: `#rrggbb`, `rrggbb` or `reset` (the terminal's own).
pub fn color(text: &str) -> Option<Color> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("reset") {
        return Some(Color::Reset);
    }
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(rgb)
}

/// `~/.local/state/quotop/preferences.toml` (or the local data directory, on
/// systems without a state directory).
pub fn default_path() -> Option<PathBuf> {
    state_path(FILE_NAME)
}

/// A file in the app's state directory, `~/.local/state/quotop/` (or the local
/// data directory, on systems without a state directory). This is where the
/// choices made in the TUI menus live.
pub fn state_path(name: &str) -> Option<PathBuf> {
    let base = dirs::state_dir().or_else(dirs::data_local_dir)?;
    Some(base.join("quotop").join(name))
}

/// Reads the saved preferences. A missing or unreadable file (or one naming a
/// theme that no longer exists) is the same as no choice: the screen writes it
/// again next time. A missing or unknown `language` keeps the current one.
pub fn read(path: &Path) -> Option<Preferences> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let theme = table.get("theme")?.as_str().and_then(Theme::index)?;
    let bar = table
        .get("bar")
        .and_then(|value| value.as_str())
        .and_then(BarStyle::from_id)
        .unwrap_or_default();
    let language = table
        .get("language")
        .and_then(|value| value.as_str())
        .and_then(i18n::index_of)
        .unwrap_or_else(i18n::current_index);
    Some(Preferences {
        language,
        theme,
        bar,
    })
}

/// Saves the preferences (creating the directory if needed). Writes to a
/// temporary file and renames it, so a half-written file never replaces the
/// good one.
pub fn save(path: &Path, preferences: &Preferences) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("toml.tmp");
    std::fs::write(&temporary, preferences.to_toml())?;
    std::fs::rename(&temporary, path)
}
