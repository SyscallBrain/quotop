//! The domain contract: `Reading` and `Meter`.
//!
//! Type, field and variant names and the serde format are **closed**: they are
//! the format of `tests/fixtures/readings_2026-09-26.json` and of the
//! `last.json` cache. Changing them means bumping [`CACHE_VERSION`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Service family; groups the sections of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Search,
    Llm,
    Subscription,
    Other,
}

/// What the service's API can tell about the balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// Exact value (money or credits).
    ExactBalance,
    /// Usage against a known limit.
    UsageVsLimit,
    /// Only the rate-limit headers.
    RateLimitOnly,
    /// There is no balance endpoint; only the key is validated.
    NoApi,
}

/// The cost of **reading** the service, not of using it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cost {
    Free,
    /// Spends the service's own quota (Brave, Context7): never read by the
    /// automatic refresh.
    PerRequest,
}

/// Unit of a meter's value. `Currency` requires `currency`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Currency,
    Credits,
    Tokens,
    Characters,
    Requests,
    Percent,
}

/// A meter's traffic light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Warning,
    Critical,
    Exhausted,
    /// A balance with no known limit: the level cannot be determined, which is
    /// not the same as "full".
    NoReference,
}

/// Status of the reading of **one service** (the level is per meter).
///
/// `expired_credential` only exists for Claude and is decided without network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    Ok,
    NoCredential,
    InvalidCredential {
        http: u16,
    },
    ExpiredCredential,
    RateLimited {
        http: u16,
    },
    /// Any other `>= 400`, or a business error inside a `200` (MiniMax).
    ApiError {
        http: u16,
        message: String,
    },
    /// Fixed transport category; the `ureq::Error` is never formatted.
    NetworkError {
        message: String,
    },
    /// Parsing the response failed; never a panic.
    UnexpectedFormat {
        message: String,
    },
    Unsupported,
}

/// One row of the screen, usually one window/quota of the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meter {
    /// "monthly plan", "5h", "credits"… in English; the screen translates it
    /// with [`crate::i18n::label`].
    pub label: String,
    pub unit: Unit,
    /// `Some` when `unit == Currency`.
    pub currency: Option<String>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    /// When `None` with `used` and `limit` present, it is `limit - used`.
    pub remaining: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub level: Level,
}

/// Everything the app knows about a service after reading it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// Stable id: "tavily", "opencode_go", "claude"…
    pub provider: String,
    /// Display name.
    pub service: String,
    pub category: Category,
    pub class: Class,
    pub cost: Cost,
    pub read_at: DateTime<Utc>,
    pub duration_ms: Option<u64>,
    pub status: Status,
    /// Empty if `status != Ok` or if `class == NoApi`.
    pub meters: Vec<Meter>,
}

/// The `~/.cache/quotop/last.json` file: `version` lets a cache from another
/// generation be refused without deleting it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub readings: Vec<Reading>,
}

/// Version of the cache format this generation writes and understands.
/// Version 1 had Portuguese field names; version 2 is the English schema.
pub const CACHE_VERSION: u32 = 2;

/// A meter's thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Thresholds {
    /// **Remaining** fraction (`remaining / limit`); 0.20 and 0.05 by default.
    Fraction { warning: f64, critical: f64 },
    /// In the meter's units, for balances with no limit.
    Absolute { warning: f64, critical: f64 },
}

/// Default warning threshold: 0.20 of the remaining fraction.
pub const DEFAULT_WARNING: f64 = 0.20;
/// Default critical threshold: 0.05 of the remaining fraction.
pub const DEFAULT_CRITICAL: f64 = 0.05;

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds::Fraction {
            warning: DEFAULT_WARNING,
            critical: DEFAULT_CRITICAL,
        }
    }
}

impl Thresholds {
    /// Short explanation for the detail panel.
    pub fn description(&self) -> String {
        match self {
            Thresholds::Fraction { warning, critical } => {
                format!("remaining fraction: warning <= {warning}, critical <= {critical}")
            }
            Thresholds::Absolute { warning, critical } => {
                format!("meter units: warning <= {warning}, critical <= {critical}")
            }
        }
    }
}

/// The level from what is known about a meter.
///
/// Rules, in this order:
/// 1. no limit (or a limit of 0): `remaining <= 0` gives `Exhausted`; absolute
///    `thresholds` compare in the meter's units; without them, `NoReference`;
/// 2. with a limit and no `remaining`: `limit - used` is computed; without
///    both, `NoReference` (a level is never made up);
/// 3. `f = remaining / limit`: `f <= 0` → `Exhausted`, `f <= critical` →
///    `Critical`, `f <= warning` → `Warning`, else `Ok`. An `f > 1` (plan
///    exceeded thanks to a bonus) gives `Ok`.
pub fn level(
    used: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    thresholds: Thresholds,
) -> Level {
    let remaining = match remaining {
        Some(r) => Some(r),
        None => match (used, limit) {
            (Some(u), Some(l)) => Some(l - u),
            _ => None,
        },
    };

    let limit = match limit {
        Some(l) if l > 0.0 => l,
        // No known limit: only a zero balance is conclusive.
        _ => {
            return match remaining {
                Some(r) if r <= 0.0 => Level::Exhausted,
                _ => match thresholds {
                    Thresholds::Absolute { warning, critical } => {
                        compare_absolute(remaining, warning, critical)
                    }
                    Thresholds::Fraction { .. } => Level::NoReference,
                },
            };
        }
    };

    let Some(r) = remaining else {
        return Level::NoReference;
    };

    match thresholds {
        Thresholds::Fraction { warning, critical } => {
            let f = r / limit;
            if f <= 0.0 {
                Level::Exhausted
            } else if f <= critical {
                Level::Critical
            } else if f <= warning {
                Level::Warning
            } else {
                Level::Ok
            }
        }
        Thresholds::Absolute { warning, critical } => compare_absolute(Some(r), warning, critical),
    }
}

/// Comparison in the meter's units, used when the service has no limit.
fn compare_absolute(remaining: Option<f64>, warning: f64, critical: f64) -> Level {
    match remaining {
        None => Level::NoReference,
        Some(r) if r <= 0.0 => Level::Exhausted,
        Some(r) if r <= critical => Level::Critical,
        Some(r) if r <= warning => Level::Warning,
        Some(_) => Level::Ok,
    }
}
