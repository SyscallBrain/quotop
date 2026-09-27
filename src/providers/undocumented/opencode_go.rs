//! OpenCode Go — **undocumented endpoint**, observed on 2026-09-26.
//!
//! `GET https://opencode.ai/zen/go/v1/usage`, with the Bearer token from
//! OpenCode's `auth.json` (`OPENCODE_GO_API_KEY`, injected by
//! `credentials::load`). Three windows: `usage.{rolling,weekly,monthly}` with
//! `percent` (0 to 100, unlike Ollama's fraction) and `resetsAt` as ISO-8601.
//!
//! The same endpoint, with no official documentation, is also read by the
//! `robinebers/openusage` project — a divergence between the two is the sign
//! that OpenCode changed the format.
//!
//! A missing or `null` window yields no meter (`if usage.get(k)` in
//! `project.py`). It is not a public endpoint: if OpenCode changes it, only
//! this row breaks.

use chrono::{DateTime, Utc};

use crate::credentials::{Credentials, VAR_OPENCODE_GO};
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, bearer, body_of, credential, field,
    number_at, optional_instant, request,
};

const URL: &str = "https://opencode.ai/zen/go/v1/usage";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://opencode.ai/zen/go";

/// The windows, in the order the TUI shows them: `rolling` is the 5-hour one.
const WINDOWS: [(&str, &str); 3] = [
    ("rolling", "5h"),
    ("weekly", "weekly"),
    ("monthly", "monthly"),
];

/// OpenCode Go: subscription read through the client's own usage endpoint.
pub struct OpenCodeGo;

impl Provider for OpenCodeGo {
    fn id(&self) -> &'static str {
        "opencode_go"
    }

    fn service(&self) -> &'static str {
        "OpenCode Go"
    }

    fn category(&self) -> Category {
        Category::Subscription
    }

    fn class(&self) -> Class {
        Class::UsageVsLimit
    }

    /// The variable is the same one `credentials::load` injects from
    /// `auth.json`: a single definition, in `credentials.rs`.
    fn variables(&self) -> &'static [&'static str] {
        &[VAR_OPENCODE_GO]
    }

    fn dashboard(&self) -> &'static str {
        DASHBOARD
    }

    fn endpoint(&self) -> Option<&'static str> {
        Some(URL)
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VAR_OPENCODE_GO)?;
        request(http, Request::get(URL, bearer(key)), cred).map(|r| vec![r])
    }

    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let usage = field(&body, "usage")?;
        if !usage.is_object() {
            return Err(bad_format("`usage` is not a JSON object"));
        }
        let mut meters = Vec::new();
        for (name, label) in WINDOWS {
            let Some(window) = usage.get(name).filter(|v| v.is_object()) else {
                continue;
            };
            let percent = number_at(window, "percent")?;
            let resets_at = optional_instant(window, "resetsAt")?;
            meters.push(
                MeterBuilder::new(label, Unit::Percent)
                    .used(percent)
                    .limit(100.0)
                    .optional_resets_at(resets_at)
                    .build(),
            );
        }
        Ok(meters)
    }
}
