//! Brave Search — the balance only exists in the *rate limit* headers.
//!
//! The 2026-09-26 probe measured, on a real search `GET`:
//!
//! ```text
//! x-ratelimit-limit: 1, 2000
//! x-ratelimit-remaining: 0, 1977
//! x-ratelimit-reset: 1, 389223
//! x-ratelimit-policy: 1;w=1, 2000;w=2592000
//! ```
//!
//! They are **lists** with one entry per window, and the last one is the
//! monthly window (`;w=2592000` = 30 days) — that is the one the TUI shows,
//! same as `p_brave` in `project.py` (`i = len(limits) - 1`). The `reset` is
//! **relative** to the moment of the request, unlike Context7's
//! `ratelimit-reset`, which is an absolute epoch.
//!
//! Reading this service spends 1 request of the quota: that is why the `cost`
//! is `PerRequest` and the automatic refresh skips it.

use chrono::{DateTime, Duration, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Cost, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, credential, first, header, header_number, request,
    require_object,
};

/// Environment variable holding the key (the probe measured that
/// `BRAVE_SEARCH_API_KEY` holds the same value; `BRAVE_API_KEY` is the one the
/// probe script used).
pub const VARIABLE: &str = "BRAVE_API_KEY";

/// The measured `GET`: the cheapest search that proves the quota (`count=1`).
const URL: &str = "https://api.search.brave.com/res/v1/web/search?q=ratatui&count=1";

/// Subscription dashboard, for the detail view. This is the console URL
/// published by the service and was **not** measured by the probe.
const DASHBOARD: &str = "https://api-dashboard.search.brave.com/app/subscriptions/subscribe";

/// Brave Search: `RateLimitOnly` — there is no balance endpoint.
pub struct Brave;

impl Provider for Brave {
    fn id(&self) -> &'static str {
        "brave"
    }

    fn service(&self) -> &'static str {
        "Brave Search"
    }

    fn category(&self) -> Category {
        Category::Search
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

    /// Without the query string: `?q=ratatui&count=1` is the probe request,
    /// which is of no interest on screen.
    fn endpoint(&self) -> Option<&'static str> {
        Some(crate::providers::without_query(URL))
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let headers = header("X-Subscription-Token", key.expose().to_string());
        request(http, Request::get(URL, headers), cred).map(|r| vec![r])
    }

    /// Reads the **last** window of the three headers. A body that is not a
    /// JSON object is `UnexpectedFormat`: the numbers in these headers can only
    /// be trusted if whoever answered really is the search API.
    fn interpret(&self, responses: &[Response], now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let response = first(responses)?;
        require_object(responses)?;
        let limit = header_number(response, "x-ratelimit-limit")?;
        let remaining = header_number(response, "x-ratelimit-remaining")?;
        let reset = header_number(response, "x-ratelimit-reset")?;
        Ok(vec![
            MeterBuilder::new("requests/month", Unit::Requests)
                .limit(limit)
                .remaining(remaining)
                // Relative to `now`, like `unix_iso(t0 + resets[i])` in
                // `project.py` — in `interpret` that is the `now` it receives.
                .resets_at(now + Duration::seconds(reset as i64))
                .build(),
        ])
    }
}
