//! The anti-leak barrier for secrets.
//!
//! Two functions, with distinct roles:
//!
//! - [`mask`] transforms text that came from outside (an API's error body)
//!   before it enters a [`crate::model::Reading`]: every loaded secret, whole or
//!   as a middle chunk, becomes [`MARKER`], control characters are removed and
//!   the text is cut at 120 characters;
//! - [`check_no_leak`] is the **last** barrier, before saving the cache or
//!   printing `--json`: it searches for each secret in the final text and
//!   rejects it.
//!
//! Neither function ever returns a secret's value — only masked text or a
//! fixed reason.
//!
//! And neither looks at values shorter than [`MIN_TO_SEARCH`] characters: a
//! trivial value (the `0` of a `SOME_TOOL_DEBUG=0`) is not a credential, and
//! searching for it would only mask innocent text.

use crate::secret::{MARKER, Secret};

/// Length of the middle chunk searched for inside a long secret.
pub const CHUNK: usize = 12;

/// Minimum length, in `char`s, of a value for the guard to look at it.
///
/// Below this the value is neither searched for nor masked: a trivial value
/// coming from a `.env` file shared with another tool (`SOME_TOOL_DEBUG=0`)
/// would match the `"duration_ms": 0` of any reading and bring down the whole
/// `--json`. A real credential is much longer than this (measured: the
/// shortest one that passes the name filter has 15 characters).
pub const MIN_TO_SEARCH: usize = 8;

/// Minimum length of a secret for its middle chunk to be looked at: below this
/// the chunk would be almost the whole secret.
pub const MIN_FOR_CHUNK: usize = 16;

/// Final cut of any text coming from outside.
pub const LIMIT: usize = 120;

/// The 12-character middle chunk of a secret, if it is long enough.
///
/// Works on `char`s, not bytes: a secret with multibyte characters must not
/// split a UTF-8 boundary (the `String::replace` that follows needs valid
/// slices).
pub fn middle_chunk(value: &str) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < MIN_FOR_CHUNK {
        return None;
    }
    let start = (chars.len() - CHUNK) / 2;
    Some(chars[start..start + CHUNK].iter().collect())
}

/// Text that is safe for a message: no secrets, no control characters, 120
/// characters at most.
///
/// The order matters: secrets are masked first (a `\n` in the middle of a key
/// does not hide it), and only then are control characters removed and the
/// text cut.
pub fn mask(text: &str, secrets: &[&Secret]) -> String {
    let mut output = text.to_string();
    for secret in secrets {
        let value = secret.expose();
        if value.chars().count() < MIN_TO_SEARCH {
            continue;
        }
        if output.contains(value) {
            output = output.replace(value, MARKER);
        }
        if let Some(chunk) = middle_chunk(value)
            && output.contains(&chunk)
        {
            output = output.replace(&chunk, MARKER);
        }
    }
    output
        .chars()
        .filter(|c| !c.is_control())
        .take(LIMIT)
        .collect()
}

/// `Err(reason)` if the text contains a loaded secret, whole or as a middle
/// chunk. The reason is a fixed sentence and contains no values.
pub fn check_no_leak(text: &str, secrets: &[&Secret]) -> Result<(), &'static str> {
    for secret in secrets {
        let value = secret.expose();
        if value.chars().count() < MIN_TO_SEARCH {
            continue;
        }
        if text.contains(value) {
            return Err("a whole secret appears in the text");
        }
        if let Some(chunk) = middle_chunk(value)
            && text.contains(&chunk)
        {
            return Err("a 12-character chunk of a secret appears in the text");
        }
    }
    Ok(())
}
