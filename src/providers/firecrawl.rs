//! Firecrawl — `GET /v2/team/credit-usage`.
//!
//! **v2** is used: v1 (`remaining_credits`, `snake_case`) also answers 200,
//! but the v2 body includes the end of the billing period, which is what gives
//! the reset date.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bearer, body_of, credential, field,
    instant_from_iso, number_at, request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "FIRECRAWL_API_KEY";
/// The team's credits endpoint.
const URL: &str = "https://api.firecrawl.dev/v2/team/credit-usage";
/// The service's dashboard.
const DASHBOARD: &str = "https://www.firecrawl.dev/app";

/// Firecrawl: credits left in the period (`camelCase`).
pub struct Firecrawl;

impl Provider for Firecrawl {
    fn id(&self) -> &'static str {
        "firecrawl"
    }

    fn service(&self) -> &'static str {
        "Firecrawl"
    }

    fn category(&self) -> Category {
        Category::Search
    }

    fn class(&self) -> Class {
        Class::ExactBalance
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

    /// `data.remainingCredits` with the plan (`data.planCredits`) as the limit
    /// and the end of the period as the reset. The remainder can be **larger**
    /// than the plan (rolled-over credits) — a fraction above 1 is `ok`.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let data = field(&body, "data")?;
        Ok(vec![
            MeterBuilder::new("credits", Unit::Credits)
                .limit(number_at(data, "planCredits")?)
                .remaining(number_at(data, "remainingCredits")?)
                .resets_at(instant_from_iso(data, "billingPeriodEnd")?)
                .build(),
        ])
    }
}
