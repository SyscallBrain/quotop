//! Composio — the format of the usage summary body is not confirmed.
//!
//! The endpoint the probe tried is `POST
//! https://backend.composio.dev/api/v3.1/project/usage/summary` with
//! `x-api-key: <key>` (`COMPOSIO_API_KEY`) and a JSON body; without a key it
//! got 401. Since both the request body and the response body are still
//! unconfirmed, the row is `Unsupported` with a credential and `NoCredential`
//! without one — and no request is spent just to always answer the same thing.
//!
//! Once the body is measured, this is the place to change: the class is
//! already `UsageVsLimit` because that is what the usage summary gives.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status};
use crate::providers::{Provider, Response, credential};

/// Environment variable holding the key.
pub const VARIABLE: &str = "COMPOSIO_API_KEY";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://app.composio.dev/";

/// Composio: project usage, not yet confirmed.
pub struct Composio;

impl Provider for Composio {
    fn id(&self) -> &'static str {
        "composio"
    }

    fn service(&self) -> &'static str {
        "Composio"
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
