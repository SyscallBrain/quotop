//! Writing a key to the key file (`keys.env`), at the request of the TUI's
//! services menu.
//!
//! It is a convenience: the file is still read as always
//! ([`crate::credentials`]), and editing it by hand is still the normal way.
//! Rules for this write:
//!
//! - only the **first** of the `key_files` in `config.toml` is written (by
//!   default `~/.config/quotop/keys.env`) — never another tool's `.env`, and
//!   never Claude Code's or OpenCode's files;
//! - the rest of the file stays as it was, byte for byte: only the variable's
//!   line changes (or is appended at the end, if it does not exist);
//! - the file always ends up with `chmod 600` (the directory, if created,
//!   `700`), and is written to a temporary file and renamed, so it is never
//!   left half-written;
//! - the value arrives as a [`Secret`] and is only exposed to go to disk.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

use crate::credentials::parse_dotenv;
use crate::secret::Secret;

/// Why a key was not saved.
#[derive(Debug)]
pub enum KeyError {
    /// The value cannot go into a `NAME=value` line (empty, with spaces or
    /// with control characters). The payload is the i18n key of the reason.
    InvalidValue(&'static str),
    /// The disk refused.
    Io(std::io::Error),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::InvalidValue(reason) => {
                let text = crate::t!("keyfile.rejected", reason = crate::i18n::text(reason));
                f.write_str(&text)
            }
            KeyError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for KeyError {}

impl From<std::io::Error> for KeyError {
    fn from(error: std::io::Error) -> Self {
        KeyError::Io(error)
    }
}

/// Checks that the value fits a `NAME=value` line without changing meaning.
/// The messages never carry the value.
pub fn validate(value: &str) -> Result<(), KeyError> {
    if value.is_empty() {
        return Err(KeyError::InvalidValue("keyfile.empty"));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(KeyError::InvalidValue("keyfile.has_spaces"));
    }
    if value.chars().any(char::is_control) {
        return Err(KeyError::InvalidValue("keyfile.has_control"));
    }
    Ok(())
}

/// Saves the variables to the key file: replaces the first line of each one
/// (the one the reader uses) or appends it at the end.
pub fn save(path: &Path, pairs: &[(&str, &Secret)]) -> Result<(), KeyError> {
    for (_, value) in pairs {
        validate(value.expose())?;
    }
    let old = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let new = with_variables(&old, pairs);

    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let temporary = path.with_extension("env.tmp");
    // `create_new` fails if a temporary file is left over from an interrupted
    // write: remove it first (it is ours, by its name).
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(new.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    Ok(())
}

/// The file's text with the variables set. A line counts as the variable's if
/// the reader's parser reads it as such (with or without `export`).
fn with_variables(old: &str, pairs: &[(&str, &Secret)]) -> String {
    let mut rows: Vec<String> = old.lines().map(str::to_string).collect();
    for (name, value) in pairs {
        let new_line = format!("{name}={}", value.expose());
        let existing = rows.iter().position(|line| {
            parse_dotenv(line)
                .first()
                .is_some_and(|(parsed, _)| parsed == name)
        });
        match existing {
            Some(index) => rows[index] = new_line,
            None => rows.push(new_line),
        }
    }
    let mut text = rows.join("\n");
    text.push('\n');
    text
}
