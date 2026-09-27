//! Interface translations.
//!
//! Every user-facing text goes through this module. Translations live in
//! `locales/<code>.toml`, one file per language, embedded in the binary at
//! compile time. A TOML table is a key prefix, so
//!
//! ```toml
//! [i18n]
//! unknown_language = "unknown language `{code}`"
//! ```
//!
//! is the key `i18n.unknown_language`. Values may contain `{name}` placeholders,
//! filled in by [`t!`](crate::t). Counted texts use two keys, `<key>.one` and
//! `<key>.other`, chosen by [`plural`].
//!
//! A key missing from a translation falls back to English (`en-US`), and a key
//! missing from English falls back to the key itself — so an incomplete
//! translation never shows an empty string. The tests in `tests/i18n.rs` check
//! that every language has exactly the English keys and placeholders, and that
//! every key used in the source code exists.
//!
//! **Adding a language**: copy `locales/en-US.toml` to `locales/<code>.toml`,
//! translate the values, and add one line to [`LANGUAGES`]. See
//! `CONTRIBUTING.md`.
//!
//! The current language is per thread (like the colour theme in the TUI), so
//! tests running in parallel can use different languages.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// A language quotop can be shown in.
pub struct Language {
    /// BCP 47 code, as written in `config.toml` (`language = "pt-PT"`).
    pub code: &'static str,
    /// The language's own name for itself, as shown in the Preferences screen.
    pub name: &'static str,
    /// The embedded `locales/<code>.toml`.
    source: &'static str,
}

/// The languages, in the order the Preferences screen lists them. The first
/// one is the default and the fallback for missing keys.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en-US",
        name: "English (US)",
        source: include_str!("../locales/en-US.toml"),
    },
    Language {
        code: "pt-PT",
        name: "Português (Portugal)",
        source: include_str!("../locales/pt-PT.toml"),
    },
];

/// Index of the default language (English) in [`LANGUAGES`].
pub const DEFAULT: usize = 0;

thread_local! {
    static CURRENT: Cell<usize> = const { Cell::new(DEFAULT) };
}

/// Makes `index` (in [`LANGUAGES`]) the language of this thread.
pub fn set_current(index: usize) {
    CURRENT.with(|current| current.set(index.min(LANGUAGES.len() - 1)));
}

/// Index of this thread's language in [`LANGUAGES`].
pub fn current_index() -> usize {
    CURRENT.with(Cell::get)
}

/// This thread's language.
pub fn current() -> &'static Language {
    &LANGUAGES[current_index()]
}

/// Index of a language by code. Case and `_`/`-` are ignored, so `pt_pt` and
/// `PT-PT` both find Portuguese.
pub fn index_of(code: &str) -> Option<usize> {
    let wanted = code.trim().replace('_', "-").to_ascii_lowercase();
    LANGUAGES
        .iter()
        .position(|language| language.code.to_ascii_lowercase() == wanted)
}

/// The language to start with: the one saved in the Preferences screen, else
/// the one in `config.toml`, else English. An unknown code in `config.toml` is
/// a warning, never an error.
pub fn resolve(
    config_language: Option<&str>,
    saved: Option<&str>,
    warnings: &mut Vec<String>,
) -> usize {
    if let Some(index) = saved.and_then(index_of) {
        return index;
    }
    let Some(code) = config_language else {
        return DEFAULT;
    };
    index_of(code).unwrap_or_else(|| {
        warnings.push(crate::t!(
            "i18n.unknown_language",
            code = code,
            languages = codes().join(", ")
        ));
        DEFAULT
    })
}

/// The codes of all languages, in order.
pub fn codes() -> Vec<&'static str> {
    LANGUAGES.iter().map(|language| language.code).collect()
}

/// Reads the `language` key of a TOML file (the saved preferences, or a
/// `config.toml` before it is fully loaded). Anything unreadable is `None`.
pub fn language_in_file(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table.get("language")?.as_str().map(str::to_string)
}

/// All catalogues, flattened to `dotted.key → text`, parsed on first use.
fn catalogues() -> &'static [HashMap<String, String>] {
    static CATALOGUES: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();
    CATALOGUES.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|language| {
                parse(language.source)
                    .unwrap_or_else(|error| panic!("locales/{}.toml: {error}", language.code))
            })
            .collect()
    })
}

/// Parses a locale file into `dotted.key → text`. Public for the tests, which
/// compare the languages key by key.
pub fn parse(source: &str) -> Result<HashMap<String, String>, String> {
    let table: toml::Table = toml::from_str(source).map_err(|error| error.to_string())?;
    let mut flat = HashMap::new();
    flatten("", &table, &mut flat)?;
    Ok(flat)
}

fn flatten(
    prefix: &str,
    table: &toml::Table,
    flat: &mut HashMap<String, String>,
) -> Result<(), String> {
    for (key, value) in table {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::String(text) => {
                flat.insert(full, text.clone());
            }
            toml::Value::Table(inner) => flatten(&full, inner, flat)?,
            _ => return Err(format!("`{full}` must be a string or a table")),
        }
    }
    Ok(())
}

/// The text of `key` in the current language, falling back to English and
/// then to the key itself.
pub fn text(key: &str) -> String {
    let catalogues = catalogues();
    catalogues[current_index()]
        .get(key)
        .or_else(|| catalogues[DEFAULT].get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// [`text`] with its `{name}` placeholders filled in.
pub fn format(key: &str, args: &[(&str, String)]) -> String {
    fill(text(key), args)
}

/// `<key>.one` when `n == 1`, `<key>.other` otherwise, with `{n}` and the other
/// placeholders filled in.
pub fn plural(key: &str, n: usize, args: &[(&str, String)]) -> String {
    let form = if n == 1 { "one" } else { "other" };
    let mut all = vec![("n", n.to_string())];
    all.extend(args.iter().cloned());
    fill(text(&format!("{key}.{form}")), &all)
}

fn fill(mut text: String, args: &[(&str, String)]) -> String {
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// A meter label in the current language. Providers write their labels in
/// English (`"monthly plan"`, `"5h"`), which is also what `--json` prints; the
/// `[labels]` table of each locale translates them for the screen. A label
/// with no translation is shown as it is.
pub fn label(english: &str) -> String {
    let key = format!("labels.{english}");
    let catalogues = catalogues();
    catalogues[current_index()]
        .get(&key)
        .cloned()
        .unwrap_or_else(|| english.to_string())
}

/// The decimal separator of the current language (`meta.decimal_separator`).
pub fn decimal_separator() -> char {
    text("meta.decimal_separator").chars().next().unwrap_or('.')
}

/// Translates a key, optionally filling `{name}` placeholders:
///
/// ```ignore
/// t!("meta.decimal_separator");
/// t!("i18n.unknown_language", code = "xx", languages = "en-US, pt-PT");
/// ```
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::text($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format($key, &[$((stringify!($name), $value.to_string())),+])
    };
}
