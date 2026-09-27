//! The cache, `dirs::cache_dir()/quotop/last.json`.
//!
//! - **atomic write**: a temporary file in the same directory, `sync_all` and
//!   `rename`. Two instances at the same time each write their own temporary
//!   file and `rename` is atomic — the last one wins, with no lock;
//! - **mode 600 from birth**: the temporary file is created with 600 already,
//!   so there is no window in which the cache is readable by others;
//! - **the anti-leak guard is the last barrier**: the text that comes out of
//!   [`prepare`] is the same that goes to the file and to `--json`, and it is
//!   that text that is searched for secrets. If there is a leak, **nothing is
//!   saved or printed**;
//! - **reading is lenient**: a missing, unreadable or invalid file, or one with
//!   an unknown `version`, gives `None` with a warning, and the file **is not
//!   deleted** — the next refresh rewrites it (nothing is lost: the cache is
//!   disposable).

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::guard;
use crate::model::{CACHE_VERSION, Cache};
use crate::secret::Secret;
use crate::t;

/// File name inside `dirs::cache_dir()/quotop/`.
pub const FILE_NAME: &str = "last.json";

/// The cache's mode: only the owner can read it.
pub const MODE: u32 = 0o600;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The cache path: `dirs::cache_dir()/quotop/last.json`.
pub fn default_path() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("quotop").join(FILE_NAME))
}

/// What prevented saving. No variant carries a secret's value: the reason for
/// a leak is the provider's name.
#[derive(Debug)]
pub enum CacheError {
    /// Serialization refused the cache (there is no expected path to this).
    Serialization { message: String },
    /// The anti-leak guard found a secret: nothing is saved.
    Leak { provider: Option<String> },
    /// I/O failure while creating, writing or renaming.
    Io { path: PathBuf, message: String },
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            CacheError::Serialization { message } => {
                t!("cache.serialization_failed", message = message)
            }
            CacheError::Leak {
                provider: Some(provider),
            } => t!("cache.leak_in", provider = provider),
            CacheError::Leak { provider: None } => t!("cache.leak"),
            CacheError::Io { path, message } => format!("{}: {message}", path.display()),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for CacheError {}

/// The cache text — the same for the file and for `--json` — already checked
/// by the anti-leak guard.
///
/// This being the only door is the guarantee that nothing is saved or printed
/// without going through the guard.
pub fn prepare(cache: &Cache, secrets: &[&Secret]) -> Result<String, CacheError> {
    let text = serde_json::to_string_pretty(cache).map_err(|e| CacheError::Serialization {
        message: e.to_string(),
    })?;
    check(&text, cache, secrets)?;
    Ok(text)
}

/// Saves a cache's `text` atomically and with mode 600.
///
/// An I/O error halfway leaves the old cache intact: only the `rename` swaps
/// the file, and the temporary file is removed.
pub fn save(path: &Path, text: &str) -> Result<(), CacheError> {
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;

    // The name carries the pid and a counter: two instances never share a
    // temporary file, and each one renames its own.
    let temporary = dir.join(format!(
        ".{FILE_NAME}.tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let result = write_file(&temporary, text)
        .and_then(|()| std::fs::rename(&temporary, path).map_err(|e| io_error(path, &e)));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Reads the cache, if it is from this generation. `None` (with a warning,
/// when there is a reason) if there is no cache or it cannot be used; the file
/// stays where it is.
pub fn read(path: &Path, secrets: &[&Secret], warnings: &mut Vec<String>) -> Option<Cache> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warnings.push(t!("cache.read_failed", path = path.display(), error = e));
            return None;
        }
    };

    let cache: Cache = match serde_json::from_str(&text) {
        Ok(cache) => cache,
        Err(e) => {
            // The `serde_json` error may quote a piece of the text it read
            // (`invalid type: string "…"`), so it goes through the guard.
            warnings.push(t!(
                "cache.unreadable",
                path = path.display(),
                error = guard::mask(&e.to_string(), secrets)
            ));
            return None;
        }
    };

    if cache.version != CACHE_VERSION {
        warnings.push(t!(
            "cache.other_version",
            path = path.display(),
            found = cache.version,
            expected = CACHE_VERSION
        ));
        return None;
    }

    Some(cache)
}

/// Writes the temporary file, with the cache's mode from the first byte.
fn write_file(path: &Path, text: &str) -> Result<(), CacheError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(MODE)
        .open(path)
        .map_err(|e| io_error(path, &e))?;
    file.write_all(text.as_bytes())
        .map_err(|e| io_error(path, &e))?;
    file.sync_all().map_err(|e| io_error(path, &e))?;
    Ok(())
}

/// The anti-leak guard over the final text and, if it catches something, which
/// reading it came from — that is what allows saying "leak prevented:
/// <provider>".
fn check(text: &str, cache: &Cache, secrets: &[&Secret]) -> Result<(), CacheError> {
    if guard::check_no_leak(text, secrets).is_ok() {
        return Ok(());
    }
    let provider = cache
        .readings
        .iter()
        .find(|reading| {
            let text = serde_json::to_string(reading).unwrap_or_default();
            guard::check_no_leak(&text, secrets).is_err()
        })
        .map(|reading| reading.provider.clone());
    Err(CacheError::Leak { provider })
}

fn io_error(path: &Path, e: &std::io::Error) -> CacheError {
    CacheError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}
