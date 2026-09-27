//! Tavily — `GET /usage`: the monthly plan.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bearer, body_of, credential, field, number_at,
    request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "TAVILY_API_KEY";
/// Usage endpoint.
const URL: &str = "https://api.tavily.com/usage";
/// The service's dashboard.
const DASHBOARD: &str = "https://app.tavily.com/home";

/// Tavily: monthly plan usage against the plan's limit.
pub struct Tavily;

impl Provider for Tavily {
    fn id(&self) -> &'static str {
        "tavily"
    }

    fn service(&self) -> &'static str {
        "Tavily"
    }

    fn category(&self) -> Category {
        Category::Search
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

    /// `account.plan_usage` over `account.plan_limit`.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let account = field(&body, "account")?;
        Ok(vec![
            MeterBuilder::new("monthly plan", Unit::Credits)
                .used(number_at(account, "plan_usage")?)
                .limit(number_at(account, "plan_limit")?)
                .build(),
        ])
    }
}
