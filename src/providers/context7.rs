//! Context7 — quota in the `ratelimit-*` headers.
//!
//! Measured on 2026-09-26: `ratelimit-limit: 1000`,
//! `ratelimit-remaining: 981`, `ratelimit-reset: 1790812800` and
//! `context7-quota-tier: free`. Unlike Brave, each field has **one** value and
//! the `reset` is an **absolute epoch** (= 2026-10-01T00:00:00Z), not an offset
//! from the request.
//!
//! Reading this service spends 1 request of the monthly quota:
//! `cost = PerRequest`.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Cost, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, bearer, credential, first,
    header_number, instant_from_epoch, request, require_object,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "CONTEXT7_API_KEY";

/// The measured `GET`: a library search, the cheapest request that proves the
/// quota.
const URL: &str = "https://context7.com/api/v2/libs/search?libraryName=ratatui&query=tui";

/// Dashboard, for the detail view (not measured by the probe; see the note on
/// the trait).
const DASHBOARD: &str = "https://context7.com/dashboard";

/// Context7: `RateLimitOnly` — there is no balance endpoint.
pub struct Context7;

impl Provider for Context7 {
    fn id(&self) -> &'static str {
        "context7"
    }

    fn service(&self) -> &'static str {
        "Context7"
    }

    fn category(&self) -> Category {
        Category::Other
    }

    fn class(&self) -> Class {
        Class::RateLimitOnly
    }

    /// Spends 1 request of the service's quota to find out how much is left.
    fn cost(&self) -> Cost {
        Cost::PerRequest
    }

    fn variables(&self) -> &'static [&'static str] {
        &[VARIABLE]
    }

    fn dashboard(&self) -> &'static str {
        DASHBOARD
    }

    /// Without the query string: `?libraryName=ratatui&query=tui` is the probe
    /// request, which is of no interest on screen.
    fn endpoint(&self) -> Option<&'static str> {
        Some(crate::providers::without_query(URL))
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        request(http, Request::get(URL, bearer(key)), cred).map(|r| vec![r])
    }

    /// The `reset` is an absolute epoch instant, not an offset: `now` does not
    /// come into it (this is `unix_iso(h["ratelimit-reset"])` in `project.py`).
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let response = first(responses)?;
        require_object(responses)?;
        let limit = header_number(response, "ratelimit-limit")?;
        let remaining = header_number(response, "ratelimit-remaining")?;
        let reset = header_number(response, "ratelimit-reset")?;
        let resets_at = instant_from_epoch(reset as i64)
            .ok_or_else(|| bad_format("the `ratelimit-reset` header is not an instant"))?;
        Ok(vec![
            MeterBuilder::new("requests/month", Unit::Requests)
                .limit(limit)
                .remaining(remaining)
                .resets_at(resets_at)
                .build(),
        ])
    }
}
