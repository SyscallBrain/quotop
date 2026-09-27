//! Pushover — documented monthly quota, still without a real body.
//!
//! `GET https://api.pushover.net/1/apps/limits.json?token=<app token>`: the
//! token goes in the **query string** (there is no authentication header, just
//! like Jina), and the expected body is
//! `{status, limit, remaining, reset}` — the `reset` as an epoch.
//!
//! The 2026-09-26 probe got 400 with an invalid token, so this body comes from
//! the documentation; the test runs against
//! `tests/fixtures/synthetic/pushover_limits.json`.
//!
//! An application token in this URL only stays out of an accidental `{:?}`
//! because the `Debug` of [`Request`] redacts the whole URL.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, body_of, credential, instant_from_epoch,
    number_at, request,
};

/// Environment variable holding the application token.
pub const VARIABLE: &str = "PUSHOVER_APP_TOKEN";

const URL: &str = "https://api.pushover.net/1/apps/limits.json";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://pushover.net/apps";

/// Pushover: messages this month against the application's limit.
pub struct Pushover;

impl Provider for Pushover {
    fn id(&self) -> &'static str {
        "pushover"
    }

    fn service(&self) -> &'static str {
        "Pushover"
    }

    fn category(&self) -> Category {
        Category::Other
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

    /// The `URL` without the `?token=` that `read` appends to it.
    fn endpoint(&self) -> Option<&'static str> {
        Some(URL)
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let url = format!("{URL}?token={}", key.expose());
        request(http, Request::get(&url, Vec::new()), cred).map(|r| vec![r])
    }

    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        // `limit` and `remaining` are the answer: without them there is no
        // meter. An API refusal arrives as a 4xx and already stopped in
        // `request`.
        let limit = number_at(&body, "limit")?;
        let remaining = number_at(&body, "remaining")?;
        let reset = number_at(&body, "reset")?;
        let resets_at = instant_from_epoch(reset as i64)
            .ok_or_else(|| bad_format("`reset` is not an epoch instant"))?;
        Ok(vec![
            MeterBuilder::new("messages/month", Unit::Requests)
                .limit(limit)
                .remaining(remaining)
                .resets_at(resets_at)
                .build(),
        ])
    }
}
