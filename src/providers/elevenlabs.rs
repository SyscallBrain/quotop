//! ElevenLabs — `GET /v1/user/subscription`.

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, body_of, credential, header, instant_from_epoch,
    number_at, request,
};

/// Environment variable holding the key.
pub const VARIABLE: &str = "ELEVENLABS_API_KEY";
/// Subscription endpoint.
const URL: &str = "https://api.elevenlabs.io/v1/user/subscription";
/// ElevenLabs' authentication header.
const AUTH_HEADER: &str = "xi-api-key";
/// The service's dashboard.
const DASHBOARD: &str = "https://elevenlabs.io/app/subscription";

/// ElevenLabs: characters spent against the plan's limit.
pub struct ElevenLabs;

impl Provider for ElevenLabs {
    fn id(&self) -> &'static str {
        "elevenlabs"
    }

    fn service(&self) -> &'static str {
        "ElevenLabs"
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

    fn endpoint(&self) -> Option<&'static str> {
        Some(URL)
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let key = credential(cred, VARIABLE)?;
        let headers = header(AUTH_HEADER, key.expose().to_string());
        request(http, Request::get(URL, headers), cred).map(|r| vec![r])
    }

    /// `character_count` over `character_limit`, with the reset as an epoch
    /// (`next_character_count_reset_unix`).
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        let body = body_of(responses)?;
        let reset = instant_from_epoch(number_at(&body, "next_character_count_reset_unix")? as i64)
            .ok_or_else(|| {
                crate::providers::bad_format("`next_character_count_reset_unix` is out of range")
            })?;
        Ok(vec![
            MeterBuilder::new("characters", Unit::Characters)
                .used(number_at(&body, "character_count")?)
                .limit(number_at(&body, "character_limit")?)
                .resets_at(reset)
                .build(),
        ])
    }
}
