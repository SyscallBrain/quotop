//! Deepgram — documented balance, in **two requests**.
//!
//! 1. `GET https://api.deepgram.com/v1/projects` → `projects[0].project_id`;
//! 2. `GET https://api.deepgram.com/v1/projects/{project_id}/balances` →
//!    `balances[].{balance_id, amount, units, purchase_order_id}`.
//!
//! Authentication is `Authorization: Token <key>` (`DEEPGRAM_API_KEY`) —
//! Deepgram does not use `Bearer` like the others.
//!
//! It is the only provider that makes two requests: `read` returns **both**
//! responses (the projects and the balances) and `interpret` reads the
//! second one. The expected body comes from the documentation; the test runs
//! against `tests/fixtures/synthetic/deepgram_*.json` and, once a key is
//! available, a measured body should replace it.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, credential, field, header, number_at,
    request, response_at, text_at,
};
use crate::secret::Secret;

/// Environment variable holding the key.
pub const VARIABLE: &str = "DEEPGRAM_API_KEY";

const PROJECTS_URL: &str = "https://api.deepgram.com/v1/projects";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://console.deepgram.com/";

/// Deepgram: exact balance, in `balances[].amount`.
pub struct Deepgram;

/// `Authorization: Token <key>` — a `Bearer` here would get a 401.
fn token_header(key: &Secret) -> Vec<(String, Secret)> {
    header("Authorization", format!("Token {}", key.expose()))
}

/// The account's first project. A project is not a secret (it is an id) and
/// goes as is into the URL of the second request.
fn project(response: &Response) -> Result<String, Status> {
    let body = response.json()?;
    let projects = field(&body, "projects")?;
    let first = projects
        .as_array()
        .and_then(|list| list.first())
        .ok_or_else(|| bad_format("the response has no project"))?;
    text_at(first, "project_id")
}

impl Provider for Deepgram {
    fn id(&self) -> &'static str {
        "deepgram"
    }

    fn service(&self) -> &'static str {
        "Deepgram"
    }

    fn category(&self) -> Category {
        Category::Other
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
        Some(PROJECTS_URL)
    }

    /// Two requests: the projects first, the balances second. Each one goes
    /// through `request`, so each gets the same status handling (401, 429,
    /// 5xx, timeout) as the single-request providers.
    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let projects = request(http, Request::get(PROJECTS_URL, token_header(key)), cred)?;
        let project = project(&projects)?;
        let url = format!("{PROJECTS_URL}/{project}/balances");
        let balances = request(http, Request::get(&url, token_header(key)), cred)?;
        Ok(vec![projects, balances])
    }

    /// Reads the **second** response (the balances). `response_at` gives
    /// `UnexpectedFormat` if it is missing — a `read` pretending to have made
    /// two requests must not pass as good.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = response_at(responses, 1)?.json()?;
        let balances = field(&body, "balances")?;
        let balances = balances
            .as_array()
            .ok_or_else(|| bad_format("`balances` is not a list"))?;
        let mut meters = Vec::new();
        for balance in balances {
            let remaining = number_at(balance, "amount")?;
            // `units` is the currency as the API gives it (`"usd"`): the TUI
            // shows it as is, without inventing a code.
            let currency = text_at(balance, "units")?;
            meters.push(
                MeterBuilder::new("balance", Unit::Currency)
                    .remaining(remaining)
                    .currency(&currency)
                    .build(),
            );
        }
        Ok(meters)
    }
}
