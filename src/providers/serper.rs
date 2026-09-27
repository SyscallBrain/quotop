//! Serper — `GET /account`: balance in credits, with no known limit.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, body_of, credential, header, number_at, request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "SERPER_API_KEY";
/// Account endpoint.
const URL: &str = "https://google.serper.dev/account";
/// Serper's authentication header.
const AUTH_HEADER: &str = "X-API-KEY";
/// The service's dashboard.
const DASHBOARD: &str = "https://serper.dev/dashboard";

/// Serper: credit balance, with no plan (the response has `rateLimit`, not a
/// credit limit).
pub struct Serper;

impl Provider for Serper {
    fn id(&self) -> &'static str {
        "serper"
    }

    fn service(&self) -> &'static str {
        "Serper"
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
        let headers = header(AUTH_HEADER, key.expose().to_string());
        request(http, Request::get(URL, headers), cred).map(|r| vec![r])
    }

    /// `balance`. Without a limit the level is `NoReference` — a balance with
    /// no limit is not "full".
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        Ok(vec![
            MeterBuilder::new("credits", Unit::Credits)
                .remaining(number_at(&body, "balance")?)
                .build(),
        ])
    }
}
