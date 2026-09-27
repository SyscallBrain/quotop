//! `Secret`: the only type that may hold a key value.
//!
//! Invariants of this barrier:
//! - `Debug` and `Display` write [`MARKER`], never the value;
//! - it does **not** implement `Serialize` (so it cannot end up in a cache, a
//!   structured log or a response);
//! - the value only comes out through `.expose()`, and the only place allowed
//!   to call it is `http.rs`, when building the request (plus the anti-leak
//!   guard, which searches for it);
//! - it does not implement `Clone`, so that duplicating a secret is explicit.

use std::fmt;

/// Placeholder for a hidden value. It is also what shows up on screen if a
/// secret is formatted by mistake.
pub const MARKER: &str = "«secret»";

/// A secret value (key, token, SID).
pub struct Secret(String);

impl Secret {
    /// Wraps a value. There is no direct access to the inside without
    /// `.expose()`.
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The plain value. **Only** for building the request in `http.rs` and for
    /// the anti-leak guard to decide whether there is a leak; never for
    /// `format!` of messages, URLs, logs or the screen.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the value is empty (a variable that is set but has no value is
    /// not a key).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MARKER)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MARKER)
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Secret::new(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Secret::new(value)
    }
}
