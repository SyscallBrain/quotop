//! Claude (subscription) — **undocumented endpoint**, observed on 2026-09-26.
//!
//! `GET https://api.anthropic.com/api/oauth/usage`, with the OAuth *access
//! token* from `~/.claude/.credentials.json` and the headers Claude Code uses.
//! It is not a public endpoint: if Anthropic changes it, the Claude row starts
//! giving `UnexpectedFormat` (or `ApiError`) and **the other 25 do not depend
//! on it**.
//!
//! Rules this file implements:
//!
//! - `expiresAt <= now` → `ExpiredCredential` **without a network request**:
//!   Claude Code renews the token by itself and it is the one in charge;
//! - the `refreshToken` is **never** used — refreshing it would rotate the
//!   token that the user's running Claude Code has open;
//! - headers `anthropic-beta: oauth-2025-04-20` and the `User-Agent`
//!   `claude-code/<version>` (by default the probe's; `claude_user_agent` in
//!   `config.toml` replaces it through [`Claude::with_user_agent`]).

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status, Unit};
use crate::providers::{
    MeterBuilder, Provider, Request, Response, bad_format, bearer, body_of, number,
    optional_instant, request, require_object,
};
use crate::secret::Secret;

/// Where the credential lives. It is not an environment variable: it is the
/// file Claude Code writes, and it is what [`Provider::variables`] returns so
/// the detail panel can show where the credential comes from.
pub const CREDENTIAL: &str = "~/.claude/.credentials.json";

/// Default `User-Agent`: the same one the 2026-09-26 probe used.
pub const DEFAULT_USER_AGENT: &str = "claude-code/2.1.282";

const URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// Header that marks the request as coming from an OAuth client.
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

/// Subscription usage page, for the detail view (not measured by the probe).
const DASHBOARD: &str = "https://claude.ai/settings/usage";

/// The windows the body carries, in the order the TUI shows them: the label is
/// ours, the field is the API's.
const WINDOWS: [(&str, &str); 4] = [
    ("five_hour", "5h"),
    ("seven_day", "7 days"),
    ("seven_day_opus", "7 days Opus"),
    ("seven_day_sonnet", "7 days Sonnet"),
];

/// Claude: subscription read through Claude Code's usage endpoint.
pub struct Claude {
    user_agent: String,
}

impl Default for Claude {
    fn default() -> Claude {
        Claude::new()
    }
}

impl Claude {
    /// The provider with the default `User-Agent`.
    pub fn new() -> Claude {
        Claude::with_user_agent(DEFAULT_USER_AGENT)
    }

    /// The same, with the `User-Agent` from `config.toml`.
    pub fn with_user_agent(user_agent: impl Into<String>) -> Claude {
        Claude {
            user_agent: user_agent.into(),
        }
    }

    /// The `User-Agent` in use (`claude_user_agent` from the config, or the
    /// default).
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }
}

impl Provider for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn service(&self) -> &'static str {
        "Claude"
    }

    fn category(&self) -> Category {
        Category::Subscription
    }

    fn class(&self) -> Class {
        Class::UsageVsLimit
    }

    fn variables(&self) -> &'static [&'static str] {
        &[CREDENTIAL]
    }

    fn dashboard(&self) -> &'static str {
        DASHBOARD
    }

    fn endpoint(&self) -> Option<&'static str> {
        Some(URL)
    }

    /// No credential → `NoCredential`; expired → `ExpiredCredential` **without
    /// touching the network**; only then is there a request.
    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let Some(claude) = cred.claude() else {
            return Err(Status::NoCredential);
        };
        if claude.is_expired(Utc::now()) {
            return Err(Status::ExpiredCredential);
        }
        let mut headers = bearer(claude.token());
        // `Request` only stores header values as `Secret` (which guarantees
        // that no accidental `{:?}` shows them). Neither of these two is a
        // secret, but they go through it like the others.
        headers.push((
            "anthropic-beta".to_string(),
            Secret::new(ANTHROPIC_BETA.to_string()),
        ));
        // ureq only adds the agent's `User-Agent` when the request does not
        // carry one: this header is what gives Claude Code its own UA.
        headers.push((
            "User-Agent".to_string(),
            Secret::new(self.user_agent.clone()),
        ));
        request(http, Request::get(URL, headers), cred).map(|r| vec![r])
    }

    /// Does not use `now`: the reset instants come in the body, as ISO-8601.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        // A body from this API is always an object: `null` with HTTP 200 is not
        // one of its responses (and `project.py` would also crash reading it).
        require_object(responses)?;
        let body = body_of(responses)?;
        let mut meters = Vec::new();
        for (field, label) in WINDOWS {
            // `v and v.get("utilization") is not None` in `project.py`: a
            // window that is missing, null or without utilization yields no
            // meter (the measured accounts have no Opus window).
            let Some(window) = body.get(field).filter(|v| v.is_object()) else {
                continue;
            };
            let Some(utilization) = window.get("utilization").filter(|v| !v.is_null()) else {
                continue;
            };
            let used = number(utilization)
                .ok_or_else(|| bad_format(format!("`{field}.utilization` is not a number")))?;
            let resets_at = optional_instant(window, "resets_at")?;
            meters.push(
                MeterBuilder::new(label, Unit::Percent)
                    .used(used)
                    .limit(100.0)
                    .optional_resets_at(resets_at)
                    .build(),
            );
        }
        // Extra usage (credits bought separately) only exists if it is enabled.
        if let Some(extra) = body.get("extra_usage").filter(|v| v.is_object())
            && extra
                .get("is_enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        {
            let mut meter = MeterBuilder::new("extra usage", Unit::Currency);
            if let Some(value) = extra.get("used_credits").and_then(number) {
                meter = meter.used(value);
            }
            if let Some(value) = extra.get("monthly_limit").and_then(number) {
                meter = meter.limit(value);
            }
            if let Some(currency) = extra.get("currency").and_then(serde_json::Value::as_str) {
                meter = meter.currency(currency);
            }
            meters.push(meter.build());
        }
        Ok(meters)
    }
}
