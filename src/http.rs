//! The single HTTP client: a shared, blocking `ureq::Agent`, with fixed
//! timeouts and **no** automatic error for 4xx/5xx.
//!
//! Invariant of this barrier: the `ureq::Error` is never formatted nor
//! propagated — what comes out of here is a **fixed category** in
//! [`Status::NetworkError`]. The reason is concrete: Jina and Pushover carry
//! the key in the *query string*, and it is not known whether `ureq`'s
//! `Display` includes the URL; when in doubt, the URL does not get through.

use std::time::{Duration, Instant};

use ureq::Agent;

use crate::model::Status;
use crate::providers::{Method, Request, Response};

/// Connection timeout of every request.
pub const CONNECT_TIMEOUT_S: u64 = 5;

/// Body read limit: 1 MiB. Protects memory from a server that returns a huge
/// body.
pub const BODY_LIMIT: u64 = 1024 * 1024;

/// Fixed category of a transport error. There is no path from here to the
/// error's `Display` nor to the request URL.
fn category(error: &ureq::Error) -> &'static str {
    match error {
        ureq::Error::Timeout(_) => "timeout",
        ureq::Error::HostNotFound => "DNS",
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::TlsRequired => "TLS",
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            "connection refused"
        }
        // Everything else (protocol, `BadUri`, failed connection, body larger
        // than the limit…) falls into the generic category.
        _ => "network",
    }
}

/// `User-Agent` of every request. Claude replaces it with its own
/// (`claude_user_agent` in `config.toml`).
pub fn user_agent() -> String {
    format!("quotop/{}", env!("CARGO_PKG_VERSION"))
}

/// The app's HTTP client.
pub struct Http {
    agent: Agent,
}

impl Http {
    /// A client with the app's `User-Agent`, a 5 s `timeout_connect` and
    /// `timeout_global` = `config.timeout_s`.
    pub fn new(timeout_s: u64) -> Http {
        Http::with_user_agent(timeout_s, &user_agent())
    }

    /// The same, with an explicit `User-Agent` (Claude needs a different one).
    pub fn with_user_agent(timeout_s: u64, user_agent: &str) -> Http {
        let config = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(CONNECT_TIMEOUT_S)))
            .timeout_global(Some(Duration::from_secs(timeout_s)))
            // 4xx/5xx arrive as `Ok(Response)`: the provider is what classifies
            // them (`status_from_http`).
            .http_status_as_error(false)
            .user_agent(user_agent)
            .build();
        Http {
            agent: Agent::new_with_config(config),
        }
    }

    /// Sends a request and returns the raw response. `Err(NetworkError)` on any
    /// transport failure, with a fixed category.
    pub fn request(&self, request: &Request) -> Result<Response, Status> {
        let start = Instant::now();

        // The two methods have different `RequestBuilder` types
        // (`WithoutBody` / `WithBody`), so there is no single path here.
        let result = match request.method {
            Method::Get => {
                let mut req = self.agent.get(&request.url);
                for (name, value) in &request.headers {
                    req = req.header(name.as_str(), value.expose());
                }
                req.call()
            }
            Method::Post => {
                let mut req = self.agent.post(&request.url);
                for (name, value) in &request.headers {
                    req = req.header(name.as_str(), value.expose());
                }
                req.send(request.body.as_deref().unwrap_or_default())
            }
        };

        let mut response = match result {
            Ok(r) => r,
            Err(e) => {
                return Err(Status::NetworkError {
                    message: category(&e).to_string(),
                });
            }
        };

        let http = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect();

        let body = match response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_string()
        {
            Ok(body) => body,
            Err(e) => {
                return Err(Status::NetworkError {
                    message: category(&e).to_string(),
                });
            }
        };

        Ok(Response {
            http,
            headers,
            body,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
