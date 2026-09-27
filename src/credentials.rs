//! Credentials: environment → `key_files` in order → the subscriptions' own
//! credential files.
//!
//! Barriers that live here:
//! - a key file with group/other bits (`mode & 0o077 != 0`) **is not read**:
//!   it produces a warning with the path and the `chmod 600` to run (for
//!   consistency, the same applies to the two subscription files);
//! - values only ever exist inside a [`Secret`];
//! - from the Claude file **only** `accessToken` and `expiresAt` are read; the
//!   `refreshToken` field is never read nor stored (refreshing it would rotate
//!   the token and could leave the user's Claude Code with a revoked
//!   credential), and the app **never** writes to the subscriptions' credential
//!   files. The only write is the one from the TUI's services menu, to the
//!   user's own key file and at their request — see [`crate::keyfile`].

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::config::Config;
use crate::secret::Secret;

/// Variable the OpenCode Go key is looked up under (`auth.json` is the
/// alternative).
pub const VAR_OPENCODE_GO: &str = "OPENCODE_GO_API_KEY";

/// Path of OpenCode's `auth.json`, relative to the home directory.
const OPENCODE_PATH: &str = ".local/share/opencode/auth.json";
/// Path of Claude Code's credentials, relative to the home directory.
const CLAUDE_PATH: &str = ".claude/.credentials.json";

/// Name fragments that make a variable count as a credential. Without this
/// filter, a variable like `USER` would become a "secret" and the anti-leak
/// guard would mask innocent text. It applies to **both** sources —
/// environment and `key_files`: a `.env` file shared with other tools brings
/// their variables along, and `SOME_TOOL_DEBUG=0` is not a key. If a provider
/// ever needs a variable outside this pattern, its name is added here.
const CREDENTIAL_PATTERN: [&str; 5] = ["KEY", "TOKEN", "SECRET", "SID", "PASS"];

/// Whether the variable name looks like a credential.
pub fn looks_like_credential(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    CREDENTIAL_PATTERN.iter().any(|p| name.contains(p))
}

/// Claude's OAuth token, with the expiry the file declares.
#[derive(Debug)]
pub struct ClaudeCredential {
    access_token: Secret,
    expires_at_ms: i64,
}

impl ClaudeCredential {
    /// The token. Only for building the `Authorization` header.
    pub fn token(&self) -> &Secret {
        &self.access_token
    }

    /// The file's `expiresAt`, in epoch milliseconds.
    pub fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }

    /// Expired (`expiresAt <= now`) → `ExpiredCredential` **without network**.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at_ms <= now.timestamp_millis()
    }
}

/// The set of loaded credentials. `Debug` is safe: each value is a
/// [`Secret`], which prints as its marker.
#[derive(Debug, Default)]
pub struct Credentials {
    values: BTreeMap<String, Secret>,
    claude: Option<ClaudeCredential>,
}

impl Credentials {
    /// A variable's value, if it exists and is not empty.
    pub fn value(&self, name: &str) -> Option<&Secret> {
        self.values.get(name).filter(|s| !s.is_empty())
    }

    pub fn has(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    pub fn claude(&self) -> Option<&ClaudeCredential> {
        self.claude.as_ref()
    }

    /// How many secrets are loaded (for the status bar; never values).
    pub fn secret_count(&self) -> usize {
        self.values.len() + usize::from(self.claude.is_some())
    }

    /// All values, for the anti-leak guard and for the tests. It is the only
    /// place in the program that hands out secrets in bulk, and the guard uses
    /// them to **search** for them, never to show them.
    pub fn all_exposed(&self) -> Vec<&Secret> {
        let mut all: Vec<&Secret> = self.values.values().collect();
        if let Some(c) = &self.claude {
            all.push(&c.access_token);
        }
        all
    }
}

/// Where the credentials are read from. It exists so that the tests can inject
/// a fake environment without touching the process environment (changing it
/// in parallel tests is a race, and in `edition 2024` it is `unsafe`).
#[derive(Debug, Clone, Default)]
pub struct Source {
    /// `(NAME, value)` pairs of the process environment, in whatever order.
    pub env: Vec<(String, String)>,
    /// `.env` files, in the `config.toml` order of precedence, with `~` already
    /// expanded.
    pub key_files: Vec<PathBuf>,
    /// The user's home directory, for the subscription files.
    pub home_dir: Option<PathBuf>,
}

impl Source {
    /// The real source: the process environment, the `key_files` of
    /// `config.toml` and `HOME`.
    pub fn from_env(config: &Config) -> Source {
        Source {
            env: std::env::vars().collect(),
            key_files: config.key_files.iter().map(|p| expand_tilde(p)).collect(),
            home_dir: dirs::home_dir(),
        }
    }
}

/// Loads the credentials from the given source, adding to `warnings`.
///
/// Precedence: environment → `key_files` in order (the first non-empty value
/// wins) → OpenCode's `auth.json` (only fills `OPENCODE_GO_API_KEY` if it is
/// still missing). `auth.json` and the Claude credentials live outside the
/// variables: Claude has its own field.
pub fn load(source: &Source, warnings: &mut Vec<String>) -> Credentials {
    let mut values: BTreeMap<String, Secret> = BTreeMap::new();

    for (name, value) in &source.env {
        if !value.is_empty() && looks_like_credential(name) {
            values.insert(name.clone(), Secret::new(value.clone()));
        }
    }

    for path in &source.key_files {
        for (name, value) in read_env_file(path, warnings) {
            // The same name filter as the environment: a `.env` file shared
            // with another tool must not fill the anti-leak guard with
            // "secrets" that are not credentials.
            if value.is_empty() || !looks_like_credential(&name) {
                continue;
            }
            // `or_insert`: the environment and earlier files take precedence.
            values.entry(name).or_insert_with(|| Secret::new(value));
        }
    }

    if !values.contains_key(VAR_OPENCODE_GO)
        && let Some(secret) = read_opencode(source.home_dir.as_deref(), warnings)
    {
        values.insert(VAR_OPENCODE_GO.to_string(), secret);
    }

    let claude = read_claude(source.home_dir.as_deref(), warnings);

    Credentials { values, claude }
}

/// Reads a `.env` file after checking its permissions. Returns the pairs found
/// (empty if it cannot or must not be read).
pub fn read_env_file(path: &Path, warnings: &mut Vec<String>) -> Vec<(String, String)> {
    let Ok(meta) = fs::metadata(path) else {
        return Vec::new(); // missing: not a problem
    };
    if !mode_acceptable(&meta, path, warnings) {
        return Vec::new();
    }
    match fs::read_to_string(path) {
        Ok(text) => parse_dotenv(&text),
        Err(e) => {
            warnings.push(crate::t!(
                "credentials.read_failed",
                path = path.display(),
                error = e
            ));
            Vec::new()
        }
    }
}

/// A file is only read if it has no group/other bits.
fn mode_acceptable(meta: &fs::Metadata, path: &Path, warnings: &mut Vec<String>) -> bool {
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 == 0 {
        return true;
    }
    warnings.push(crate::t!(
        "credentials.open_permissions",
        path = path.display(),
        mode = format!("{mode:03o}")
    ));
    false
}

/// Minimal dotenv parser: `NAME=value`, `export NAME=value`, single or double
/// quotes around the value, empty lines and comment lines (`#` in the first
/// position). A `#` inside the value is **not** a comment: in a key, cutting
/// there would corrupt the value.
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = match line.strip_prefix("export") {
            Some(rest) if rest.starts_with([' ', '\t']) => rest.trim_start(),
            _ => line,
        };
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !valid_name(name) {
            continue;
        }
        pairs.push((name.to_string(), unquote(value.trim())));
    }
    pairs
}

/// A dotenv name must start with a letter or `_` and have no spaces.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Removes a pair of matching quotes around the value, if there is one.
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if value.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[value.len() - 1] == bytes[0]
    {
        return value[1..value.len() - 1].to_string();
    }
    value.to_string()
}

/// Expands a leading `~` to the user's home directory (paths in `config.toml`).
pub fn expand_tilde(path: &str) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return PathBuf::from(path);
    };
    if path == "~" {
        return home;
    }
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

/// OpenCode's `auth.json` → `["opencode-go"]["key"]`.
fn read_opencode(home: Option<&Path>, warnings: &mut Vec<String>) -> Option<Secret> {
    let path = home?.join(OPENCODE_PATH);
    let json = read_json(&path, warnings)?;
    let key = json.get("opencode-go")?.get("key")?.as_str()?;
    if key.is_empty() {
        return None;
    }
    Some(Secret::new(key))
}

/// Claude Code's credentials: `claudeAiOauth.{accessToken, expiresAt}`. The
/// rest of the file is ignored — in particular the refresh field, which is
/// never read, stored or used.
fn read_claude(home: Option<&Path>, warnings: &mut Vec<String>) -> Option<ClaudeCredential> {
    let path = home?.join(CLAUDE_PATH);
    let json = read_json(&path, warnings)?;
    let oauth = json.get("claudeAiOauth");
    let token = oauth
        .and_then(|o| o.get("accessToken"))
        .and_then(Value::as_str);
    let expires = oauth
        .and_then(|o| o.get("expiresAt"))
        .and_then(Value::as_i64);
    match (token, expires) {
        (Some(t), Some(e)) if !t.is_empty() => Some(ClaudeCredential {
            access_token: Secret::new(t),
            expires_at_ms: e,
        }),
        _ => {
            warnings.push(crate::t!(
                "credentials.claude_unusable",
                path = path.display()
            ));
            None
        }
    }
}

/// Reads and parses a JSON file after checking its permissions. A missing file
/// is not a warning (the subscriptions may not exist); invalid JSON is.
fn read_json(path: &Path, warnings: &mut Vec<String>) -> Option<Value> {
    let meta = fs::metadata(path).ok()?;
    if !mode_acceptable(&meta, path, warnings) {
        return None;
    }
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            warnings.push(crate::t!(
                "credentials.read_failed",
                path = path.display(),
                error = e
            ));
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            warnings.push(crate::t!(
                "credentials.invalid_json",
                path = path.display(),
                error = e
            ));
            None
        }
    }
}
