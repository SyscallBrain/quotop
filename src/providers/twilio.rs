//! Twilio — `GET /2010-04-01/Accounts/{SID}/Balance.json`.
//!
//! Two quirks of the response:
//!
//! - `balance` comes **quoted** (`"8.1257"`): decimal strings must be
//!   accepted, and [`number`](crate::providers::number) does that;
//! - the `SID` goes in the URL **path**, so the URL contains a secret (just
//!   like Jina). SIDs are `AC` + 32 hex digits, they need no
//!   *percent-encoding*, and the URL is never formatted after being built.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, basic, body_of, credential, request,
};

/// Environment variable holding the account SID.
pub const SID_VARIABLE: &str = "TWILIO_ACCOUNT_SID";
/// Environment variable holding the auth token.
pub const TOKEN_VARIABLE: &str = "TWILIO_AUTH_TOKEN";
/// Prefix of the balance endpoint; the SID goes in the middle.
const URL_PREFIX: &str = "https://api.twilio.com/2010-04-01/Accounts/";
const URL_SUFFIX: &str = "/Balance.json";
/// The service's dashboard.
const DASHBOARD: &str = "https://console.twilio.com/";

/// Twilio: cash balance, with no limit.
pub struct Twilio;

impl Provider for Twilio {
    fn id(&self) -> &'static str {
        "twilio"
    }

    fn service(&self) -> &'static str {
        "Twilio"
    }

    fn category(&self) -> Category {
        Category::Other
    }

    fn class(&self) -> Class {
        Class::ExactBalance
    }

    fn variables(&self) -> &'static [&'static str] {
        &[SID_VARIABLE, TOKEN_VARIABLE]
    }

    fn dashboard(&self) -> &'static str {
        DASHBOARD
    }

    /// The account URL **without the `SID`**: the `SID` identifies the account
    /// and the screen is a surface people take screenshots of. What remains
    /// identifies the service and the API version.
    fn endpoint(&self) -> Option<&'static str> {
        Some(URL_PREFIX)
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let sid = credential(cred, SID_VARIABLE)?;
        let token = credential(cred, TOKEN_VARIABLE)?;
        let url = format!("{URL_PREFIX}{}{URL_SUFFIX}", sid.expose());
        request(http, Request::get(&url, basic(sid, token)), cred).map(|r| vec![r])
    }

    /// `balance` (a string) with the response's own `currency`.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let currency = crate::providers::text_at(&body, "currency")?;
        Ok(vec![
            MeterBuilder::new("balance", Unit::Currency)
                .remaining(crate::providers::number_at(&body, "balance")?)
                .currency(&currency)
                .build(),
        ])
    }
}
