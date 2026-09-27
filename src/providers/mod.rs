//! The [`Provider`] trait and what all providers share.
//!
//! Each of the 26 services has its own module, and all of them implement:
//!
//! - `read`, which makes 1 or 2 requests with the [`Http`] client and the
//!   `Credentials` and returns the raw responses — or the error [`Status`];
//! - `interpret`, **pure**: no network and no clock besides the `now` it
//!   receives, it turns raw responses into `Meter`s. This is what the golden
//!   tests exercise.
//!
//! No provider formats a `ureq::Error` or a URL; HTTP classification lives in
//! one place, [`status_from_http`].
//!
//! Note for the engine: levels come out of here with the default thresholds,
//! because `interpret` receives no configuration. The
//! `[thresholds.<provider>]` tables in `config.toml` are applied afterwards, on
//! the meters, by the engine.

use std::fmt;

use chrono::{DateTime, Timelike, Utc};
use serde_json::Value;

use crate::credentials::Credentials;
use crate::guard;
use crate::http::Http;
use crate::model::{self, Category, Class, Cost, Meter, Status, Thresholds, Unit};
use crate::secret::Secret;

mod brave;
mod composio;
mod context7;
mod deepgram;
mod deepseek;
mod elevenlabs;
mod exa;
mod fal;
mod firecrawl;
mod github;
mod jina;
mod minimax;
mod openrouter;
mod pushover;
mod serper;
mod tavily;
mod twilio;
mod validation;
mod x;
mod xai;

/// The three **undocumented** endpoints: Claude, OpenCode Go and Ollama.
///
/// They live in a module of their own, one file each, so that the origin of
/// the endpoint and the risk of it changing sit at the top of each file instead
/// of being diluted among the services that have public documentation.
mod undocumented {
    pub mod claude;
    pub mod ollama;
    pub mod opencode_go;
}

use brave::Brave;
use composio::Composio;
use context7::Context7;
use deepgram::Deepgram;
use deepseek::DeepSeek;
use elevenlabs::ElevenLabs;
use exa::Exa;
use fal::Fal;
use firecrawl::Firecrawl;
use github::GitHub;
use jina::Jina;
use minimax::MiniMax;
use openrouter::OpenRouter;
use pushover::Pushover;
use serper::Serper;
use tavily::Tavily;
use twilio::Twilio;
use undocumented::{ollama::Ollama, opencode_go::OpenCodeGo};
use validation::ValidationProvider;
use x::XApi;
use xai::Xai;

/// Method of a request. Only `GET` and `POST`: every endpoint used is one of
/// these two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// A request to a service.
///
/// The `Debug` output is **redacted**: the `url` may contain a secret (Jina and
/// Pushover carry the key in the *query string*, Twilio carries the SID in the
/// path), so it never shows in clear text, not even by accident in a `{:?}`.
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, Secret)>,
    pub body: Option<String>,
}

impl Request {
    /// A `GET` with the given headers (each value wrapped in a [`Secret`]).
    pub fn get(url: &str, headers: Vec<(String, Secret)>) -> Request {
        Request {
            method: Method::Get,
            url: url.to_string(),
            headers,
            body: None,
        }
    }

    /// A `POST` with a body. No provider uses it yet: the only `POST` endpoint
    /// is Composio's, which stays `Unsupported` until both the request body and
    /// the response body have been measured.
    pub fn post(url: &str, headers: Vec<(String, Secret)>, body: String) -> Request {
        Request {
            method: Method::Post,
            url: url.to_string(),
            headers,
            body: Some(body),
        }
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &"«redacted»")
            .field("headers", &names)
            .field(
                "body",
                &self.body.as_ref().map(|b| format!("{} bytes", b.len())),
            )
            .finish()
    }
}

/// The raw response to a request, as `interpret` receives it.
///
/// It holds no secrets: the response headers of these APIs do not carry any,
/// and the body only goes into a message after passing through the
/// [`guard`].
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub http: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub duration_ms: u64,
}

impl Response {
    /// Value of a response header (names compared case-insensitively, as HTTP
    /// requires).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body as JSON. A body that is not valid JSON gives
    /// `UnexpectedFormat` — never a panic; the message is fixed and does not
    /// repeat the body.
    pub fn json(&self) -> Result<Value, Status> {
        serde_json::from_str(&self.body)
            .map_err(|_| bad_format("the response body is not valid JSON"))
    }
}

/// A service whose balance, credits or quotas are read.
pub trait Provider: Send + Sync {
    /// Stable id: "tavily", "opencode_go", "claude"…
    fn id(&self) -> &'static str;
    /// Name shown on screen.
    fn service(&self) -> &'static str;
    fn category(&self) -> Category;
    fn class(&self) -> Class;
    /// Cost of **reading** the service; `PerRequest` services never take part
    /// in the automatic refresh.
    fn cost(&self) -> Cost {
        Cost::Free
    }
    /// Environment variables this service needs (all required).
    ///
    /// Claude is the exception: its credential does not come from the
    /// environment, and what this list holds is the **path of the file** it
    /// comes from — that is what the detail panel shows, and it is the only
    /// row like that.
    fn variables(&self) -> &'static [&'static str];
    /// URL of the service's dashboard, for the detail view (never holds
    /// secrets).
    ///
    /// These URLs are the console URLs published by the services themselves
    /// and were **not** measured by the 2026-09-26 probe, unlike everything
    /// else in the providers.
    fn dashboard(&self) -> &'static str;
    /// Endpoint the balance is read from — what the detail view shows, always
    /// **without a query string** (providers that have one use
    /// [`without_query`]) and never with credential values in the path.
    ///
    /// It comes from the provider — not from a table in the TUI — so there is a
    /// single source of truth for the URL: it is the same `const` that `read`
    /// uses. `None` for providers that define no request at all, which are
    /// exactly the ones that answer `Unsupported` without touching the
    /// network.
    fn endpoint(&self) -> Option<&'static str> {
        None
    }
    /// Makes 1 or 2 requests. `Err` when the credential is missing or the HTTP
    /// status is an error.
    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status>;
    /// **Pure**: no network, no clock besides `now`. This is what the golden
    /// tests exercise against the 2026-09-26 projection.
    fn interpret(&self, responses: &[Response], now: DateTime<Utc>) -> Result<Vec<Meter>, Status>;
}

/// The part of a URL before the `?`.
///
/// It is what [`Provider::endpoint`] returns for providers that carry data in
/// the *query string* (Brave, Context7 and fal.ai put the query or the
/// `expand` there): such an endpoint must never bring a secret to the screen,
/// which is a surface people take screenshots of.
pub fn without_query(url: &str) -> &str {
    url.split_once('?').map_or(url, |(base, _)| base)
}

/// Ids of the providers whose endpoint is **undocumented**.
///
/// It sits next to the declaration of the `undocumented` module because it is
/// the list of what is inside it: the `†` on screen and the README note come
/// from here.
pub const UNDOCUMENTED: [&str; 3] = ["claude", "opencode_go", "ollama"];

// ---------------------------------------------------------------- HTTP → Status

/// The `http → Status` rule, same as `status_of` in
/// `tests/fixtures/project.py`: `None` when the response is usable.
///
/// `ApiError` comes out of here without a message; whoever has the body at
/// hand calls [`request`], which fills it in, already masked.
pub fn status_from_http(http: u16) -> Option<Status> {
    match http {
        401 | 403 => Some(Status::InvalidCredential { http }),
        429 => Some(Status::RateLimited { http }),
        h if h >= 400 => Some(Status::ApiError {
            http: h,
            message: String::new(),
        }),
        _ => None,
    }
}

/// The credential of a variable, or `NoCredential`.
///
/// It is the first step of every `read`: without a credential there is no
/// request — not a single one. The value never leaves here in clear text: only
/// as a [`Secret`].
pub fn credential<'a>(cred: &'a Credentials, name: &str) -> Result<&'a Secret, Status> {
    cred.value(name).ok_or(Status::NoCredential)
}

/// Makes a request and classifies the HTTP status.
///
/// On a `>= 400` response the body goes into the `ApiError` message **already
/// masked** and cut at 120 characters; 401/403 and 429 carry only the code.
pub fn request(http: &Http, req: Request, cred: &Credentials) -> Result<Response, Status> {
    let response = http.request(&req)?;
    match status_from_http(response.http) {
        None => Ok(response),
        Some(Status::ApiError { http, .. }) => {
            let secrets = cred.all_exposed();
            Err(Status::ApiError {
                http,
                message: guard::mask(&response.body, &secrets),
            })
        }
        Some(status) => Err(status),
    }
}

// ---------------------------------------------------------------- body helpers

/// `UnexpectedFormat` with a message of **our own**. Every message in this
/// module is built from field names in the code, never from the response
/// text.
pub fn bad_format(message: impl Into<String>) -> Status {
    Status::UnexpectedFormat {
        message: message.into(),
    }
}

/// The first response of a `read`. A well-behaved provider never returns zero
/// responses.
pub fn first(responses: &[Response]) -> Result<&Response, Status> {
    responses
        .first()
        .ok_or_else(|| bad_format("the provider returned no response"))
}

/// The JSON body of the first response.
pub fn body_of(responses: &[Response]) -> Result<Value, Status> {
    first(responses)?.json()
}

/// The response at the given index. Deepgram, which makes two requests, is the
/// only one that needs this: `response_at(&r, 1)` refuses to make up the
/// second response that `read` should have brought.
pub fn response_at(responses: &[Response], index: usize) -> Result<&Response, Status> {
    responses.get(index).ok_or_else(|| {
        bad_format(format!(
            "the provider did not return response {}",
            index + 1
        ))
    })
}

/// Checks that the first response carries a JSON body that is an **object**.
///
/// Brave and Context7 do not use the body (their balance is in the headers),
/// but they validate it: numbers taken from headers can only be trusted if
/// whoever answered really is the API we think it is.
pub fn require_object(responses: &[Response]) -> Result<(), Status> {
    if body_of(responses)?.is_object() {
        Ok(())
    } else {
        Err(bad_format("the response body is not a JSON object"))
    }
}

/// Required node of a JSON object.
pub fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, Status> {
    v.get(name)
        .ok_or_else(|| bad_format(format!("the response has no `{name}`")))
}

/// Number from a node: accepts a JSON number and a **decimal string** (Twilio
/// returns `"8.1257"` quoted).
pub fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Required number in a field.
pub fn number_at(v: &Value, name: &str) -> Result<f64, Status> {
    number(field(v, name)?).ok_or_else(|| bad_format(format!("`{name}` is not a number")))
}

/// Required text in a field.
pub fn text_at(v: &Value, name: &str) -> Result<String, Status> {
    field(v, name)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| bad_format(format!("`{name}` is not text")))
}

/// Instant from an ISO-8601 date in a field, **to the second** — same as
/// `iso_norm` in `project.py`, which produced the fixture (`…12:18:25.660Z`
/// becomes `…12:18:25Z`).
pub fn instant_from_iso(v: &Value, name: &str) -> Result<DateTime<Utc>, Status> {
    let text = text_at(v, name)?;
    instant_from_text(&text).ok_or_else(|| bad_format(format!("`{name}` is not an ISO-8601 date")))
}

/// Instant from ISO-8601 text, truncated to the second.
pub fn instant_from_text(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .and_then(|d| d.with_nanosecond(0))
}

/// Instant from epoch seconds (`unix_iso` in `project.py`).
pub fn instant_from_epoch(seconds: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(seconds, 0)
}

/// **Optional** instant in a field: missing or `null` gives `None` (the API did
/// not say when the reset is), present but unreadable gives `UnexpectedFormat`
/// — a date is never made up, and one that cannot be understood is never
/// swallowed.
pub fn optional_instant(v: &Value, name: &str) -> Result<Option<DateTime<Utc>>, Status> {
    match v.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => instant_from_iso(v, name).map(Some),
    }
}

// --------------------------------------------------------------------- headers

/// Number from a **response** header.
///
/// A list header (`"1, 2000"`, Brave) gives the **last** element: it is the
/// longest window, the monthly one (`i = len(limits) - 1` in `project.py`), and
/// that is the one the TUI shows. Context7, which has a single value, goes
/// through the same path.
pub fn header_number(response: &Response, name: &str) -> Result<f64, Status> {
    let text = response
        .header(name)
        .ok_or_else(|| bad_format(format!("the response has no `{name}` header")))?;
    text.rsplit_once(',')
        .map_or(text, |(_, last)| last)
        .trim()
        .parse::<f64>()
        .map_err(|_| bad_format(format!("the `{name}` header is not a number")))
}

/// A header with a built value. The value is kept inside a [`Secret`].
pub fn header(name: &str, value: String) -> Vec<(String, Secret)> {
    vec![(name.to_string(), Secret::new(value))]
}

/// `Authorization: Bearer <secret>`.
pub fn bearer(secret: &Secret) -> Vec<(String, Secret)> {
    header("Authorization", format!("Bearer {}", secret.expose()))
}

/// `Authorization: Basic <base64(user:password)>` (Twilio).
pub fn basic(user: &Secret, password: &Secret) -> Vec<(String, Secret)> {
    let pair = format!("{}:{}", user.expose(), password.expose());
    header(
        "Authorization",
        format!("Basic {}", base64(pair.as_bytes())),
    )
}

/// Standard Base64 (RFC 4648, with `=` padding), for Twilio's `Basic`
/// authentication. Written by hand because the dependency list is kept closed
/// and `ureq` does not re-export it.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let b0 = u32::from(group.first().copied().unwrap_or(0));
        let b1 = group.get(1).copied().map(u32::from).unwrap_or(0);
        let b2 = group.get(2).copied().map(u32::from).unwrap_or(0);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        let indices = [
            triple >> 18,
            (triple >> 12) & 63,
            (triple >> 6) & 63,
            triple & 63,
        ];
        for (position, index) in indices.iter().enumerate() {
            // 3 bytes → 4 symbols, 2 → 3, 1 → 2; the rest is `=`.
            if position <= group.len() {
                out.push(ALPHABET[*index as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ----------------------------------------------------------------------- meters

/// Builds a [`Meter`], mirroring `meter` in `project.py`:
///
/// - a missing `remaining`, with `used` and `limit` present, is
///   `limit - used`, rounded to 6 decimal places;
/// - the level comes from [`model::level`] with the default thresholds (the
///   thresholds from the configuration are applied later, by the engine).
pub struct MeterBuilder {
    label: String,
    unit: Unit,
    currency: Option<String>,
    used: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
}

impl MeterBuilder {
    pub fn new(label: &str, unit: Unit) -> MeterBuilder {
        MeterBuilder {
            label: label.to_string(),
            unit,
            currency: None,
            used: None,
            limit: None,
            remaining: None,
            resets_at: None,
        }
    }

    pub fn used(mut self, value: f64) -> MeterBuilder {
        self.used = Some(value);
        self
    }

    pub fn limit(mut self, value: f64) -> MeterBuilder {
        self.limit = Some(value);
        self
    }

    pub fn remaining(mut self, value: f64) -> MeterBuilder {
        self.remaining = Some(value);
        self
    }

    pub fn currency(mut self, value: &str) -> MeterBuilder {
        self.currency = Some(value.to_string());
        self
    }

    pub fn resets_at(mut self, value: DateTime<Utc>) -> MeterBuilder {
        self.resets_at = Some(value);
        self
    }

    /// The same, with a reset the API may not have given.
    pub fn optional_resets_at(mut self, value: Option<DateTime<Utc>>) -> MeterBuilder {
        self.resets_at = value;
        self
    }

    pub fn build(self) -> Meter {
        let remaining = self.remaining.or_else(|| match (self.used, self.limit) {
            (Some(used), Some(limit)) => Some(round6(limit - used)),
            _ => None,
        });
        Meter {
            label: self.label,
            unit: self.unit,
            currency: self.currency,
            used: self.used,
            limit: self.limit,
            remaining,
            resets_at: self.resets_at,
            level: model::level(self.used, self.limit, remaining, Thresholds::default()),
        }
    }
}

/// `round(x, 6)` in `project.py`, so the computed `remaining` matches the
/// fixture to the last digit.
fn round6(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

// --------------------------------------------------------------------- registry

/// The provider registry, in order: search → LLM → subscriptions → other.
///
/// These are the **26** services, in the exact order of `PROVIDERS` in
/// `project.py` — that is the order of the readings in the fixture and the
/// order of the cards on screen; the golden test compares it with the fixture,
/// id by id.
///
/// Note for the engine: Claude comes out of here with the default
/// `User-Agent`. To honor `claude_user_agent` from `config.toml`, the engine
/// replaces this entry with [`Claude::with_user_agent`] — neither the registry
/// nor the trait receives configuration.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        // Search.
        Box::new(Tavily),
        Box::new(Firecrawl),
        Box::new(Exa),
        Box::new(Serper),
        Box::new(Brave),
        Box::new(Jina),
        // LLMs and subscriptions.
        Box::new(OpenRouter),
        Box::new(OpenCodeGo),
        Box::new(Claude::new()),
        Box::new(DeepSeek),
        Box::new(MiniMax),
        Box::new(Xai),
        Box::new(ValidationProvider::groq()),
        Box::new(ValidationProvider::gemini()),
        Box::new(ValidationProvider::mistral()),
        Box::new(ValidationProvider::cerebras()),
        Box::new(Ollama),
        // Other.
        Box::new(ElevenLabs),
        Box::new(Deepgram),
        Box::new(Fal),
        Box::new(Composio),
        Box::new(Context7),
        Box::new(Twilio),
        Box::new(Pushover),
        Box::new(GitHub),
        Box::new(XApi),
    ]
}

/// The `User-Agent` the providers use (Claude replaces it).
pub use crate::http::user_agent;

/// Claude is the only **public** provider: `all()` builds it with the default
/// `User-Agent`, and it is through the type (not the registry) that the engine
/// swaps it for `claude_user_agent` from `config.toml` — the rest of the
/// undocumented module stays private, because only the registry uses it.
pub use undocumented::claude::Claude;
