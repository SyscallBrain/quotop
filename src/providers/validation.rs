//! Key validation for the `NoApi` services: Groq, Gemini, Mistral and
//! Cerebras.
//!
//! None of these four has a balance endpoint. What the app can say is that the
//! key **works**, using the free `/models` request, and point to the
//! dashboard:
//!
//! ```text
//! valid key · no balance API (dashboard)
//! ```
//!
//! That is why `interpret` returns **zero meters**: there is no number that
//! could be shown without making it up. A body that is not valid JSON, even
//! with HTTP 200, is `UnexpectedFormat` — otherwise the validation would stop
//! proving what it claims to prove.
//!
//! Adding another service like these takes just one more constructor: the
//! logic is all the same and `read` only differs in the authentication header
//! (Bearer or named).

use chrono::{DateTime, Utc};

use crate::credentials::Credentials;
use crate::http::Http;
use crate::model::{Category, Class, Meter, Status};
use crate::providers::{
    Provider, Request, Response, bad_format, bearer, body_of, credential, header, request,
};

/// How this service authenticates — the only difference between the four.
enum Auth {
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// A header of its own: `x-goog-api-key` for Gemini.
    Named(&'static str),
}

/// A provider that only validates the key.
pub struct ValidationProvider {
    id: &'static str,
    service: &'static str,
    /// The key's variable, and the only one this provider declares.
    variables: &'static [&'static str],
    url: &'static str,
    auth: Auth,
    dashboard: &'static str,
}

impl ValidationProvider {
    /// Groq — `GET /openai/v1/models`, Bearer.
    pub fn groq() -> ValidationProvider {
        ValidationProvider {
            id: "groq",
            service: "Groq",
            variables: &["GROQ_API_KEY"],
            url: "https://api.groq.com/openai/v1/models",
            auth: Auth::Bearer,
            dashboard: "https://console.groq.com/keys",
        }
    }

    /// Gemini — `GET /v1beta/models?pageSize=1`, with the key in the
    /// `x-goog-api-key` header (the variable is named `GOOGLE_API_KEY`).
    pub fn gemini() -> ValidationProvider {
        ValidationProvider {
            id: "gemini",
            service: "Gemini",
            variables: &["GOOGLE_API_KEY"],
            url: "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1",
            auth: Auth::Named("x-goog-api-key"),
            dashboard: "https://aistudio.google.com/apikey",
        }
    }

    /// Mistral — `GET /v1/models`, Bearer.
    pub fn mistral() -> ValidationProvider {
        ValidationProvider {
            id: "mistral",
            service: "Mistral",
            variables: &["MISTRAL_API_KEY"],
            url: "https://api.mistral.ai/v1/models",
            auth: Auth::Bearer,
            dashboard: "https://console.mistral.ai/",
        }
    }

    /// Cerebras — `GET /v1/models`, Bearer.
    pub fn cerebras() -> ValidationProvider {
        ValidationProvider {
            id: "cerebras",
            service: "Cerebras",
            variables: &["CEREBRAS_API_KEY"],
            url: "https://api.cerebras.ai/v1/models",
            auth: Auth::Bearer,
            dashboard: "https://cloud.cerebras.ai/",
        }
    }
}

impl Provider for ValidationProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn service(&self) -> &'static str {
        self.service
    }

    fn category(&self) -> Category {
        Category::Llm
    }

    fn class(&self) -> Class {
        Class::NoApi
    }

    fn variables(&self) -> &'static [&'static str] {
        self.variables
    }

    fn dashboard(&self) -> &'static str {
        self.dashboard
    }

    /// Without the query string: Gemini's `?pageSize=1` belongs to the
    /// validation request, not to the screen.
    fn endpoint(&self) -> Option<&'static str> {
        Some(crate::providers::without_query(self.url))
    }

    fn read(&self, http: &Http, cred: &Credentials) -> Result<Vec<Response>, Status> {
        let name = *self
            .variables
            .first()
            .ok_or_else(|| bad_format("validation provider without a declared variable"))?;
        let key = credential(cred, name)?;
        let headers = match self.auth {
            Auth::Bearer => bearer(key),
            Auth::Named(name) => header(name, key.expose().to_string()),
        };
        request(http, Request::get(self.url, headers), cred).map(|r| vec![r])
    }

    /// Validates the body and returns no meters: the request proves that the
    /// key works, not how much balance there is.
    fn interpret(&self, responses: &[Response], _now: DateTime<Utc>) -> Result<Vec<Meter>, Status> {
        if body_of(responses)?.is_null() {
            return Err(bad_format("the validation body is `null`"));
        }
        Ok(Vec::new())
    }
}
