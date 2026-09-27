//! `~/.config/quotop/config.toml`.
//!
//! Rules: the file is **optional**; an unknown key is a warning and is
//! ignored; invalid TOML (syntax or type) makes the app refuse to start, with
//! the line of the error, and `main` exits with code 2. This file belongs to
//! the user and the app never writes to it (the choices made in the
//! Preferences screen go elsewhere — see [`crate::tui::theme`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::Thresholds;

/// File name inside `dirs::config_dir()/quotop/`.
pub const FILE_NAME: &str = "config.toml";

const TOP_LEVEL_KEYS: [&str; 10] = [
    "language",
    "interval_min",
    "timeout_s",
    "key_files",
    "disabled",
    "claude_user_agent",
    "thresholds",
    "theme",
    "bar",
    "colors",
];

const THRESHOLD_KEYS: [&str; 2] = ["warn_below", "critical_below"];

/// A provider's thresholds, as written in the file. Both fields are optional;
/// converting them into [`Thresholds`] never makes up a threshold the user did
/// not write.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct FileThresholds {
    #[serde(default)]
    pub warn_below: Option<f64>,
    #[serde(default)]
    pub critical_below: Option<f64>,
}

/// The app's effective configuration, with the documented defaults.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Config {
    /// Minutes between automatic refreshes; `0` turns them off.
    #[serde(default = "default_interval_min")]
    pub interval_min: u32,
    /// Overall timeout of each request, in seconds.
    #[serde(default = "default_timeout_s")]
    pub timeout_s: u64,
    /// `.env` files in order of precedence (the first non-empty value wins).
    #[serde(default = "default_key_files")]
    pub key_files: Vec<String>,
    /// Provider ids to hide.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// `User-Agent` that the Claude usage endpoint requires (the default is the
    /// value that worked in the 2026-09-26 probe).
    #[serde(default = "default_claude_user_agent")]
    pub claude_user_agent: String,
    /// Per-provider thresholds, in the meter's units.
    #[serde(default)]
    pub thresholds: BTreeMap<String, FileThresholds>,
    /// Interface language (`en-US`, `pt-PT`, …); the choice saved in the
    /// Preferences screen takes precedence (see [`crate::i18n::resolve`]).
    #[serde(default)]
    pub language: Option<String>,
    /// Screen theme (`tokyo-night`, `catppuccin-mocha`, …); the choice saved in
    /// the Preferences screen takes precedence (see [`crate::tui::theme`]).
    #[serde(default)]
    pub theme: Option<String>,
    /// Bar style (see [`crate::tui::theme`] for the accepted names).
    #[serde(default)]
    pub bar: Option<String>,
    /// Theme colors to override, role → `#rrggbb` (validated by
    /// [`crate::tui::theme`]).
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
}

fn default_interval_min() -> u32 {
    15
}

fn default_timeout_s() -> u64 {
    10
}

fn default_key_files() -> Vec<String> {
    vec!["~/.config/quotop/keys.env".to_string()]
}

fn default_claude_user_agent() -> String {
    "claude-code/2.1.282".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            interval_min: default_interval_min(),
            timeout_s: default_timeout_s(),
            key_files: default_key_files(),
            disabled: Vec::new(),
            claude_user_agent: default_claude_user_agent(),
            thresholds: BTreeMap::new(),
            language: None,
            theme: None,
            bar: None,
            colors: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Thresholds for a provider: the file's, if any, else the default
    /// remaining-fraction ones.
    ///
    /// With only `warn_below`, there is no absolute critical level (`critico`
    /// is 0, which no positive balance reaches): a threshold the user did not
    /// write is never made up. With only `critical_below`, `warning` equals it
    /// (below it is critical, above it is `Ok`, with no warning zone).
    pub fn thresholds_for(&self, provider: &str) -> Thresholds {
        let Some(f) = self.thresholds.get(provider) else {
            return Thresholds::default();
        };
        if f.warn_below.is_none() && f.critical_below.is_none() {
            return Thresholds::default();
        }
        let critical = f.critical_below.unwrap_or(0.0);
        let warning = f.warn_below.unwrap_or(critical);
        Thresholds::Absolute { warning, critical }
    }
}

/// The result of reading the configuration file.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub config: Config,
    /// Warnings for the status bar/stderr (they never contain key values).
    pub warnings: Vec<String>,
    /// The file that was read; `None` when there is none (all defaults).
    pub origin: Option<PathBuf>,
}

/// Why startup was refused.
#[derive(Debug)]
pub enum ConfigError {
    /// The file exists but could not be read.
    Read { path: PathBuf, message: String },
    /// Invalid TOML: syntax or value type.
    Toml {
        path: PathBuf,
        line: Option<usize>,
        message: String,
    },
}

impl ConfigError {
    /// The (1-based) line of the error, when it is known.
    pub fn line(&self) -> Option<usize> {
        match self {
            ConfigError::Read { .. } => None,
            ConfigError::Toml { line, .. } => *line,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            ConfigError::Read { path, .. } | ConfigError::Toml { path, .. } => path,
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            ConfigError::Read { path, message } => {
                crate::t!(
                    "config.read_failed",
                    path = path.display(),
                    message = message
                )
            }
            ConfigError::Toml {
                path,
                line: Some(line),
                message,
            } => crate::t!(
                "config.invalid_at_line",
                path = path.display(),
                line = line,
                message = message
            ),
            ConfigError::Toml {
                path,
                line: None,
                message,
            } => crate::t!("config.invalid", path = path.display(), message = message),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for ConfigError {}

/// `dirs::config_dir()/quotop/config.toml`, if there is a configuration
/// directory.
pub fn default_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("quotop").join(FILE_NAME))
}

/// Reads the default configuration file. Without one, returns the defaults,
/// with no warnings and no error.
pub fn load() -> Result<Loaded, ConfigError> {
    match default_path() {
        Some(path) if path.is_file() => load_from(&path),
        _ => Ok(Loaded {
            config: Config::default(),
            warnings: Vec::new(),
            origin: None,
        }),
    }
}

/// Reads a specific `config.toml` (what the tests use).
pub fn load_from(path: &Path) -> Result<Loaded, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;

    // First pass: validates the TOML and finds the unknown keys to warn about.
    let table: toml::Table = toml::from_str(&text).map_err(|e| toml_error(path, &text, &e))?;
    let mut warnings = Vec::new();
    unknown_keys(&table, &mut warnings);

    // Second pass: typed, so that the error carries the line of a key with the
    // wrong type (`toml` gives the `span` in both cases).
    let config: Config = toml::from_str(&text).map_err(|e| toml_error(path, &text, &e))?;

    Ok(Loaded {
        config,
        warnings,
        origin: Some(path.to_path_buf()),
    })
}

fn toml_error(path: &Path, text: &str, e: &toml::de::Error) -> ConfigError {
    ConfigError::Toml {
        path: path.to_path_buf(),
        line: e.span().map(|s| line_of_byte(text, s.start)),
        message: e.message().to_string(),
    }
}

/// The (1-based) line a byte index falls on.
fn line_of_byte(text: &str, byte: usize) -> usize {
    let end = byte.min(text.len());
    text[..end].matches('\n').count() + 1
}

/// Warns (and only warns) about the keys the configuration does not know.
/// `serde` ignores them by itself; this is only so that the user learns about a
/// typo instead of wondering why a setting did not take effect.
fn unknown_keys(table: &toml::Table, warnings: &mut Vec<String>) {
    for key in table.keys() {
        if !TOP_LEVEL_KEYS.contains(&key.as_str()) {
            warnings.push(crate::t!("config.unknown_key", file = FILE_NAME, key = key));
        }
    }
    if let Some(thresholds) = table.get("thresholds").and_then(|v| v.as_table()) {
        for (provider, value) in thresholds {
            match value.as_table() {
                Some(inner) => {
                    for key in inner.keys() {
                        if !THRESHOLD_KEYS.contains(&key.as_str()) {
                            warnings.push(crate::t!(
                                "config.unknown_threshold_key",
                                file = FILE_NAME,
                                provider = provider,
                                key = key
                            ));
                        }
                    }
                }
                None => warnings.push(crate::t!(
                    "config.threshold_not_table",
                    file = FILE_NAME,
                    provider = provider
                )),
            }
        }
    }
}
