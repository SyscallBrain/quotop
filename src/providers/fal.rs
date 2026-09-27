//! fal.ai — credits, with a documented body that is still to be measured.
//!
//! `GET https://api.fal.ai/v1/account/billing?expand=credits`, with
//! `Authorization: Key <key>` — fal.ai does not use `Bearer` (`FAL_KEY`). The
//! expected body is `credits.{current_balance, currency}`.
//!
//! The test runs against `tests/fixtures/synthetic/fal_billing.json`; once a
//! key is available, a measured body should replace it.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, body_of, credential, field, header, number_at,
    request, text_at,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "FAL_KEY";

/// The `GET` the probe tried (it got 401 without a key).
const URL: &str = "https://api.fal.ai/v1/account/billing?expand=credits";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://fal.ai/dashboard/billing";

/// fal.ai: exact balance, in credits.
pub struct Fal;

impl Provider for Fal {
    fn id(&self) -> &'static str {
        "fal"
    }

    fn service(&self) -> &'static str {
        "fal.ai"
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

    /// Without the query string: `?expand=credits` is part of the probe request.
    fn endpoint(&self) -> Option<&'static str> {
        Some(crate::providers::without_query(URL))
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let headers = header("Authorization", format!("Key {}", key.expose()));
        request(http, Request::get(URL, headers), cred).map(|r| vec![r])
    }

    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let credits = field(&body, "credits")?;
        let remaining = number_at(credits, "current_balance")?;
        let currency = text_at(credits, "currency")?;
        Ok(vec![
            MeterBuilder::new("credits", Unit::Currency)
                .remaining(remaining)
                .currency(&currency)
                .build(),
        ])
    }
}
