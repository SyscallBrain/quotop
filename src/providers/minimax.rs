//! MiniMax — the endpoint only exists for the *Token Plan*.
//!
//! `GET https://www.minimax.io/v1/token_plan/remains`, Bearer `MINIMAX_API_KEY`.
//! The 2026-09-26 probe (without a key) got a **200 with a business error
//! inside**:
//!
//! ```json
//! {"base_resp": {"status_code": 1004, "status_msg": "login fail: ..."}}
//! ```
//!
//! A business error inside a `200` is the only case where a provider may
//! deviate from the common HTTP → status rule — and it is `read` that does it,
//! with the API's text masked by the guard. With a valid body there are no
//! numbers to show (the user's key is pay-as-you-go, and for that there is no
//! documented `remains`), so `interpret` answers `Unsupported`.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::credentials::Credentials;
use crate::guard;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status};
use crate::providers::{
    Provider, Request, Response, bearer, body_of, credential, field, first, number_at, request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "MINIMAX_API_KEY";

const URL: &str = "https://www.minimax.io/v1/token_plan/remains";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://platform.minimax.io/";

/// Our own message for a MiniMax refusal: `interpret` is pure and does not
/// have the secrets needed to mask `status_msg`, so it copies no API text.
const REFUSAL: &str = "MiniMax refused the request";

/// MiniMax: `NoApi` — the *Token Plan* quota exists, the pay-as-you-go balance
/// is not public.
pub struct MiniMax;

/// The only place in the whole registry where text coming from the API goes
/// into a [`Status`]: it goes through the guard first.
fn business_error(response: &Response, base: &Value, cred: &Credentials) -> Status {
    let message = base
        .get("status_msg")
        .and_then(Value::as_str)
        .unwrap_or(REFUSAL);
    Status::ApiError {
        http: response.http,
        message: guard::mask(message, &cred.all_exposed()),
    }
}

impl Provider for MiniMax {
    fn id(&self) -> &'static str {
        "minimax"
    }

    fn service(&self) -> &'static str {
        "MiniMax"
    }

    fn category(&self) -> Category {
        Category::Llm
    }

    fn class(&self) -> Class {
        Class::NoApi
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

    /// Two steps in a single `read`: `request` handles the HTTP and, if the
    /// body carries a nonzero `base_resp.status_code`, an `ApiError` comes out
    /// with the text masked.
    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let response = request(http, Request::get(URL, bearer(key)), cred)?;
        let body = response.json()?;
        let base = field(&body, "base_resp")?;
        if number_at(base, "status_code")? != 0.0 {
            return Err(business_error(&response, base, cred));
        }
        Ok(vec![response])
    }

    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let base = field(&body, "base_resp")?;
        if number_at(base, "status_code")? != 0.0 {
            return Err(Status::ApiError {
                http: first(responses)?.http,
                message: REFUSAL.to_string(),
            });
        }
        Err(Status::Unsupported)
    }
}
