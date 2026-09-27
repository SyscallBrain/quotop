//! Jina — `GET /api/v1/api_key/fe_user?api_key=<key>`.
//!
//! This is the case that forces secret-safe URLs: the key goes **in the query
//! string**, so the request URL contains a secret. Consequences, all
//! deliberate:
//!
//! - the URL is built here, with [`Secret::expose`](crate::secret::Secret::expose),
//!   and never formatted again — the `Debug` of
//!   [`Request`](crate::providers::Request) shows a placeholder instead of the
//!   URL;
//! - a transport error gives a fixed category, because there is no telling
//!   whether `ureq`'s `Display` would include the URL.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, body_of, credential, field, number_at, request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "JINA_API_KEY";
/// The user's balance endpoint, without the key.
const URL: &str = "https://embeddings-dashboard-api.jina.ai/api/v1/api_key/fe_user";
/// The service's dashboard.
const DASHBOARD: &str = "https://jina.ai/api-dashboard/";

/// Jina: token balance, with no known limit.
pub struct Jina;

impl Provider for Jina {
    fn id(&self) -> &'static str {
        "jina"
    }

    fn service(&self) -> &'static str {
        "Jina"
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

    /// The `URL` without the `?api_key=` that `read` appends to it.
    fn endpoint(&self) -> Option<&'static str> {
        Some(URL)
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let url = format!("{URL}?api_key={}", key.expose());
        request(http, Request::get(&url, Vec::new()), cred).map(|r| vec![r])
    }

    /// `wallet.total_balance` (tokens). `regular_balance` is 0 on the measured
    /// account: the balance is the trial one, which is what the API returns as
    /// the total.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let wallet = field(&body, "wallet")?;
        Ok(vec![
            MeterBuilder::new("tokens", Unit::Tokens)
                .remaining(number_at(wallet, "total_balance")?)
                .build(),
        ])
    }
}
