//! DeepSeek — documented balance, still **without a real body**.
//!
//! `GET https://api.deepseek.com/user/balance`, Bearer `DEEPSEEK_API_KEY`. The
//! 2026-09-26 probe had no key (401 without authentication), so the expected
//! body comes from the documentation
//! (<https://api-docs.deepseek.com/api/get-user-balance>): `is_available` and
//! `balance_infos[].{currency, total_balance, granted_balance,
//! topped_up_balance}`, with the **numbers as decimal strings** (`"110.00"`).
//!
//! The test runs against `tests/fixtures/synthetic/deepseek_balance.json`;
//! once a key is available, a measured body should replace it and the test can
//! become a golden test.
//!
//! Each `balance_infos` entry is one currency: DeepSeek accounts in CNY and in
//! USD, and the TUI shows both (like `p_twilio` does with Twilio's balance).

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, bearer, body_of, credential, field,
    number_at, request, text_at,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "DEEPSEEK_API_KEY";

const URL: &str = "https://api.deepseek.com/user/balance";

/// Dashboard, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://platform.deepseek.com/usage";

/// DeepSeek: exact balance, in `balance_infos[]`.
pub struct DeepSeek;

impl Provider for DeepSeek {
    fn id(&self) -> &'static str {
        "deepseek"
    }

    fn service(&self) -> &'static str {
        "DeepSeek"
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

    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let infos = field(&body, "balance_infos")?;
        let infos = infos
            .as_array()
            .ok_or_else(|| bad_format("`balance_infos` is not a list"))?;
        let mut meters = Vec::new();
        for info in infos {
            // `total_balance` arrives quoted: `number_at` accepts the decimal
            // string and rejects anything else.
            let remaining = number_at(info, "total_balance")?;
            let currency = text_at(info, "currency")?;
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
