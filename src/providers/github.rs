//! GitHub — `GET /rate_limit`.
//!
//! The endpoint is special: checking the rate limit does not count against the
//! rate limit, so this provider is `Free` even though its class is
//! `RateLimitOnly`.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bearer, body_of, credential, field,
    instant_from_epoch, number_at, request,
};

/// Environment variable holding the token.
pub const VARIABLE: &str = "GITHUB_TOKEN";
/// Rate limit endpoint.
const URL: &str = "https://api.github.com/rate_limit";
/// The service's dashboard.
const DASHBOARD: &str = "https://github.com/settings/tokens";

/// GitHub: hourly request window of the REST API (`resources.core`).
pub struct GitHub;

impl Provider for GitHub {
    fn id(&self) -> &'static str {
        "github"
    }

    fn service(&self) -> &'static str {
        "GitHub"
    }

    fn category(&self) -> Category {
        Category::Other
    }

    fn class(&self) -> Class {
        Class::RateLimitOnly
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

    /// `resources.core.used` over `resources.core.limit`. The response also
    /// has `remaining`, which is left to be computed instead: a single source
    /// for the remainder across all providers.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let core = field(field(&body, "resources")?, "core")?;
        let reset = instant_from_epoch(number_at(core, "reset")? as i64)
            .ok_or_else(|| crate::providers::bad_format("`reset` is out of range"))?;
        Ok(vec![
            MeterBuilder::new("core/hour", Unit::Requests)
                .used(number_at(core, "used")?)
                .limit(number_at(core, "limit")?)
                .resets_at(reset)
                .build(),
        ])
    }
}
