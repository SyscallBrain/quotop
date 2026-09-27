//! xAI — the balance requires the **management** key and the body format is
//! not confirmed.
//!
//! The 2026-09-26 probe measured the two known paths:
//!
//! - `GET https://api.x.ai/v1/api-key` with the regular key → 401;
//! - `GET https://management-api.x.ai/v1/billing/teams/{team_id}/prepaid/balance`
//!   with `XAI_MANAGEMENT_KEY` → 400 ("Invalid uuid") for lack of a `team_id`.
//!
//! Without a `team_id` and without a confirmed body, no meter is made up: with
//! a credential the row is `Unsupported`, without one `NoCredential`. The
//! endpoint exists (the class is `ExactBalance`), but no request is spent just
//! to always answer the same thing — once the body is measured, this is the
//! place to change.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status};
use crate::providers::{Provider, Response, credential};

/// Environment variable holding the management key.
pub const VARIABLE: &str = "XAI_MANAGEMENT_KEY";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://console.x.ai/team/default/billing";

/// xAI: prepaid balance, not yet confirmed.
pub struct Xai;

impl Provider for Xai {
    fn id(&self) -> &'static str {
        "xai"
    }

    fn service(&self) -> &'static str {
        "xAI"
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

    fn read(&self, _http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        credential(cred, VARIABLE)?;
        Err(Status::Unsupported)
    }

    fn interpret(
        &self,
        _responses: &[Response],
        _now: DateTime<Utc>,
    ) -> Result<Vec<Meter>, Status> {
        Err(Status::Unsupported)
    }
}
