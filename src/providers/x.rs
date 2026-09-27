//! X API — the usage endpoint exists; it is the body format that is not
//! confirmed.
//!
//! `GET https://api.x.com/2/usage/tweets` with `Authorization: Bearer
//! <X_BEARER_TOKEN>`; the probe got 401 with an outdated token, and the
//! service's class is `UsageVsLimit` (100 reads per month on the basic plan).
//!
//! Without a valid credential to confirm the body, the row is `Unsupported`
//! with a credential and `NoCredential` without one — the same treatment xAI
//! and Composio get. Once a token is available, this is the place to change.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status};
use crate::providers::{Provider, Response, credential};

/// Environment variable holding the Bearer token.
pub const VARIABLE: &str = "X_BEARER_TOKEN";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://developer.x.com/en/portal/dashboard";

/// X API: reads used this month, not yet confirmed.
pub struct XApi;

impl Provider for XApi {
    fn id(&self) -> &'static str {
        "x"
    }

    fn service(&self) -> &'static str {
        "X API"
    }

    fn category(&self) -> Category {
        Category::Other
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
