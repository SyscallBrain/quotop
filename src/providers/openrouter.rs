//! OpenRouter — `GET /api/v1/credits`.
//!
//! `/api/v1/key` (also measured) gives daily/weekly/monthly usage, but no
//! limit usable in a meter: that belongs in the detail panel, not here.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bearer, body_of, credential, field, number_at,
    request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "OPENROUTER_API_KEY";
/// Credits endpoint.
const URL: &str = "https://openrouter.ai/api/v1/credits";
/// Currency of OpenRouter's amounts.
const CURRENCY: &str = "USD";
/// The service's dashboard.
const DASHBOARD: &str = "https://openrouter.ai/settings/credits";

/// OpenRouter: credits bought versus credits spent, in USD.
pub struct OpenRouter;

impl Provider for OpenRouter {
    fn id(&self) -> &'static str {
        "openrouter"
    }

    fn service(&self) -> &'static str {
        "OpenRouter"
    }

    fn category(&self) -> Category {
        Category::Llm
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

    /// `data.total_usage` over `data.total_credits`. The remainder is computed
    /// (0.43 USD in the measurement: 1.4% of the plan → critical).
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let data = field(&body, "data")?;
        Ok(vec![
            MeterBuilder::new("credits", Unit::Currency)
                .used(number_at(data, "total_usage")?)
                .limit(number_at(data, "total_credits")?)
                .currency(CURRENCY)
                .build(),
        ])
    }
}
