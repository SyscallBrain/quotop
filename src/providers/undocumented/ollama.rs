//! Ollama (cloud) — **undocumented endpoint**, observed on 2026-09-26.
//!
//! `GET https://ollama.com/api/usage`, Bearer `OLLAMA_API_KEY`. The measured
//! body has `limits.{session,weekly}.{usage, models}` with `usage` as a
//! **fraction** (0.001 = 0.1%) — unlike OpenCode Go's `percent` (0 to 100) —
//! and **no reset date**. The measured account has no `monthly` window.
//!
//! The same endpoint, with no official documentation, is also read by the
//! `robinebers/openusage` project — a divergence between the two is the sign
//! that Ollama changed the format.
//!
//! A window the account does not have is not shown; one that is present but
//! has no `usage` is `UnexpectedFormat`. It is not a public endpoint: if Ollama
//! changes it, only this row breaks.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, bearer, body_of, credential, field,
    number_at, request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "OLLAMA_API_KEY";

const URL: &str = "https://ollama.com/api/usage";

/// Keys/account page, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://ollama.com/settings";

/// The windows, in the order the body gives them and the TUI shows them.
const WINDOWS: [(&str, &str); 3] = [
    ("session", "session"),
    ("weekly", "weekly"),
    ("monthly", "monthly"),
];

/// Ollama: the plan's usage limits (subscription).
pub struct Ollama;

/// The API's fraction as a percentage, rounded to 3 decimal places like
/// `round(usage * 100, 3)` in `project.py` (0.001 → 0.1).
fn percentage(fraction: f64) -> f64 {
    (fraction * 100.0 * 1000.0).round() / 1000.0
}

impl Provider for Ollama {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn service(&self) -> &'static str {
        "Ollama Cloud"
    }

    fn category(&self) -> Category {
        Category::Llm
    }

    fn class(&self) -> Class {
        Class::UsageVsLimit
    }

    fn variables(&self) -> &'static [&'static str] {
        &[VARIABLE]
    }

    fn dashboard(&self) -> &'static str {
        DASHBOARD
    }

    fn endpoint(&self) -> Option<&'static str> {
        Some(URL)
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        request(http, Request::get(URL, bearer(key)), cred).map(|r| vec![r])
    }

    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let limits = field(&body, "limits")?;
        if !limits.is_object() {
            return Err(bad_format("`limits` is not a JSON object"));
        }
        let mut meters = Vec::new();
        for (name, label) in WINDOWS {
            // `if k in limits` in `project.py`: what the account does not have
            // is not zero.
            let Some(window) = limits.get(name) else {
                continue;
            };
            let fraction = number_at(window, "usage")?;
            meters.push(
                MeterBuilder::new(label, Unit::Percent)
                    .used(percentage(fraction))
                    .limit(100.0)
                    .build(),
            );
        }
        Ok(meters)
    }
}
