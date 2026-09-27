//! Exa — no balance endpoint with the search key.
//!
//! The 2026-09-26 probe tried `GET https://admin-api.exa.ai/team-management/api-keys`
//! with the regular key and got **401** `{"error":"Unauthorized"}`: that route
//! belongs to the admin API, which requires its own *service key*, and there is
//! no balance route that accepts the search key.
//!
//! There is no point spending a request just to get the same 401: the provider
//! answers `Unsupported` without touching the network, and the TUI shows that
//! there is no balance API, with the dashboard link.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status};
use crate::providers::{Provider, Response};

/// Environment variable holding the key (it is the service that cannot use it
/// for the balance).
pub const VARIABLE: &str = "EXA_API_KEY";
/// The service's dashboard — the only place where Exa's balance is visible.
const DASHBOARD: &str = "https://dashboard.exa.ai";

/// Exa: `NoApi`, a measured decision, not a missing key.
pub struct Exa;

impl Provider for Exa {
    fn id(&self) -> &'static str {
        "exa"
    }

    fn service(&self) -> &'static str {
        "Exa"
    }

    fn category(&self) -> Category {
        Category::Search
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

    /// No request: the only known read route answered 401 to this key.
    fn read(&self, _http: &Http, _cred: &Credentials) -> Result<Vec<Response>, Status> {
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
