//! Golden tests of the providers against the projection measured on
//! 2026-09-26.
//!
//! `tests/fixtures/readings_2026-09-26.json` is what the real probe produced
//! (projected by `project.py`); `tests/fixtures/probe_2026-09-26.json` holds the
//! HTTP responses of those probes, already sanitized. These tests check that
//! `interpret` in Rust reproduces the same meters from the same responses —
//! with no network at all: `interpret` is pure.

use chrono::Utc;
use serde_json::Value;

use quotop::credentials::Credentials;
use quotop::http::Http;
use quotop::model::{Cache, Class, Meter, Reading, Status};
use quotop::providers::{Provider, Response, all, status_from_http};

const PROBE: &str = include_str!("fixtures/probe_2026-09-26.json");
const READINGS: &str = include_str!("fixtures/readings_2026-09-26.json");

/// Tolerance when comparing numbers: the fixture went through `json` +
/// `round(x, 6)`.
const TOLERANCE: f64 = 1e-6;

/// provider id → id of the probe that measured it, in registry order.
const PROBES: &[(&str, &str)] = &[
    ("tavily", "tavily_usage"),
    ("firecrawl", "firecrawl_v2"),
    ("exa", "exa_admin_keys"),
    ("serper", "serper_account"),
    ("brave", "brave_search"),
    ("jina", "jina_fe_user"),
    ("openrouter", "openrouter_credits"),
    ("opencode_go", "opencode_go_usage"),
    ("claude", "claude_oauth_usage"),
    ("deepseek", "deepseek_balance"),
    ("minimax", "minimax_token_plan"),
    ("xai", "xai_mgmt_prepaid"),
    ("groq", "groq_models"),
    ("gemini", "gemini_models"),
    ("mistral", "mistral_models"),
    ("cerebras", "cerebras_models"),
    ("ollama", "ollama_usage"),
    ("elevenlabs", "elevenlabs_sub"),
    ("deepgram", "deepgram_projects"),
    ("fal", "fal_billing"),
    ("composio", "composio_usage"),
    ("context7", "context7_search"),
    ("twilio", "twilio_balance"),
    ("pushover", "pushover_limits"),
    ("github", "github_rate_limit"),
    ("x", "x_usage_tweets"),
];

// ------------------------------------------------------------------- helpers

fn probe_run() -> Value {
    serde_json::from_str(PROBE).expect("the fixture's probe run is JSON")
}

fn cache() -> Cache {
    serde_json::from_str(READINGS).expect("the fixture's projection is a `Cache`")
}

fn provider(id: &str) -> Box<dyn Provider> {
    all()
        .into_iter()
        .find(|p| p.id() == id)
        .unwrap_or_else(|| panic!("provider `{id}` is not in all()"))
}

fn reading(provider_id: &str) -> Reading {
    cache()
        .readings
        .into_iter()
        .find(|r| r.provider == provider_id)
        .unwrap_or_else(|| panic!("the fixture has no reading for `{provider_id}`"))
}

/// A provider's probe, as a `Value`, from the probe id.
fn probe(probe_id: &str) -> Value {
    probe_run()["probes"]
        .as_array()
        .expect("`probes` is a list")
        .iter()
        .find(|p| p["id"] == probe_id)
        .unwrap_or_else(|| panic!("the fixture has no probe `{probe_id}`"))
        .clone()
}

/// The probe rebuilt as a [`Response`], as `http.rs` would deliver it.
fn probe_response(probe: &Value) -> Response {
    let r = &probe["response"];
    let headers = r["headers"]
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    Response {
        http: r["http"].as_u64().expect("the probe has `http`") as u16,
        headers,
        body: serde_json::to_string(&r["body"]).expect("the probe body is JSON"),
        duration_ms: r["ms"].as_u64().unwrap_or(0),
    }
}

/// One case per provider: identity, classification, status and meters.
fn check(provider_id: &str) {
    let expected = reading(provider_id);
    let provider = provider(provider_id);

    // Identity and classification: what the TUI shows must be what was
    // measured.
    assert_eq!(provider.service(), expected.service, "{provider_id}");
    assert_eq!(provider.category(), expected.category, "{provider_id}");
    assert_eq!(provider.class(), expected.class, "{provider_id}");
    assert_eq!(provider.cost(), expected.cost, "{provider_id}");
    assert!(
        !provider.variables().is_empty(),
        "{provider_id}: every provider declares the variable it uses"
    );
    assert!(
        provider.dashboard().starts_with("https://"),
        "{provider_id}: the dashboard is a URL"
    );

    let probe_id = PROBES
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, probe_id)| *probe_id)
        .unwrap_or_else(|| panic!("no probe mapped for `{provider_id}`"));
    let probe = probe(probe_id);
    let present = probe["credential_present"]
        .as_bool()
        .expect("the probe says whether there was a credential");

    let now = cache().generated_at;

    if !present {
        // Without a credential there is no request at all: `read` refuses
        // before touching the network. That is the case of Mistral and
        // Cerebras in this measurement.
        assert_eq!(expected.status, Status::NoCredential, "{provider_id}");
        assert!(expected.meters.is_empty(), "{provider_id}");
        let empty = Credentials::default();
        assert_eq!(
            provider.read(&Http::new(1), &empty),
            Err(Status::NoCredential),
            "{provider_id}: without a credential the request is not even built"
        );
        return;
    }

    let response = probe_response(&probe);
    match status_from_http(response.http) {
        Some(status) => {
            // The probe spent the request and got an HTTP error: the reading
            // must show that status and no meters.
            assert_eq!(expected.status, status, "{provider_id}: HTTP status");
            assert!(
                expected.meters.is_empty(),
                "{provider_id}: an error projects no meters"
            );
        }
        None => {
            assert_eq!(expected.status, Status::Ok, "{provider_id}");
            let meters = provider.interpret(&[response], now).unwrap_or_else(|e| {
                panic!("{provider_id}: interpreting the measured response: {e:?}")
            });
            compare(&meters, &expected.meters, provider_id);
        }
    }
}

/// Compares meters with the fixture's tolerance: text, unit, currency, dates
/// and level equal; numbers within 1e-6.
fn compare(actual: &[Meter], expected: &[Meter], context: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{context}: number of meters ({actual:?})"
    );
    for (got, want) in actual.iter().zip(expected) {
        let label = &want.label;
        assert_eq!(got.label, want.label, "{context}");
        assert_eq!(got.unit, want.unit, "{context}/{label}");
        assert_eq!(got.currency, want.currency, "{context}/{label}: currency");
        assert_eq!(
            got.resets_at, want.resets_at,
            "{context}/{label}: resets_at"
        );
        assert_eq!(got.level, want.level, "{context}/{label}: level");
        close(got.used, want.used, &format!("{context}/{label}: used"));
        close(got.limit, want.limit, &format!("{context}/{label}: limit"));
        close(
            got.remaining,
            want.remaining,
            &format!("{context}/{label}: remaining"),
        );
    }
}

fn close(actual: Option<f64>, expected: Option<f64>, field: &str) {
    match (actual, expected) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!(
            (a - b).abs() <= TOLERANCE,
            "{field}: {a} != {b} (tolerance {TOLERANCE})"
        ),
        _ => panic!("{field}: {actual:?} != {expected:?}"),
    }
}

// ------------------------------------------------------------------ registry

#[test]
fn registry_has_no_duplicates_and_follows_the_fixture_order() {
    let ids: Vec<&str> = all().iter().map(|p| p.id()).collect();

    // All 26: not one fewer (a missing row on screen) nor one more.
    assert_eq!(ids.len(), 26, "the registry must have all 26 providers");

    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "repeated ids in all(): {ids:?}");

    let expected: Vec<&str> = PROBES.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, expected, "the order of all() follows the fixture");

    // Every registered provider has a reading in the projection and a
    // measured probe.
    let cache = cache();
    for id in &ids {
        assert!(
            cache.readings.iter().any(|r| r.provider == *id),
            "`{id}` has no reading in the fixture"
        );
    }
}

// ---------------------------------------------------------------- one by one

#[test]
fn tavily_reproduces_the_projection() {
    check("tavily");
}

#[test]
fn firecrawl_reproduces_the_projection() {
    check("firecrawl");
}

#[test]
fn serper_reproduces_the_projection() {
    check("serper");
}

#[test]
fn jina_reproduces_the_projection() {
    check("jina");
}

#[test]
fn openrouter_reproduces_the_projection() {
    check("openrouter");
}

#[test]
fn elevenlabs_reproduces_the_projection() {
    check("elevenlabs");
}

#[test]
fn twilio_reproduces_the_projection() {
    check("twilio");
}

#[test]
fn github_reproduces_the_projection() {
    check("github");
}

#[test]
fn brave_reproduces_the_projection() {
    check("brave");
}

#[test]
fn context7_reproduces_the_projection() {
    check("context7");
}

#[test]
fn claude_reproduces_the_projection() {
    check("claude");
}

#[test]
fn opencode_go_reproduces_the_projection() {
    check("opencode_go");
}

#[test]
fn ollama_reproduces_the_projection() {
    check("ollama");
}

#[test]
fn deepseek_reproduces_the_projection() {
    check("deepseek");
}

#[test]
fn deepgram_reproduces_the_projection() {
    check("deepgram");
}

#[test]
fn fal_reproduces_the_projection() {
    check("fal");
}

#[test]
fn pushover_reproduces_the_projection() {
    check("pushover");
}

#[test]
fn minimax_reproduces_the_projection() {
    check("minimax");
}

/// Groq, Gemini, Mistral and Cerebras: the same parameterized provider. The
/// fixture's status is the measured one (Groq and Gemini with a key → `ok` and
/// no meters; Mistral and Cerebras without a key → `no_credential`).
#[test]
fn validation_reproduces_the_projection() {
    for id in ["groq", "gemini", "mistral", "cerebras"] {
        check(id);
    }
    // With no balance API, the row cannot carry meters.
    let reading = reading("groq");
    assert_eq!(reading.status, Status::Ok);
    assert!(
        reading.meters.is_empty(),
        "validation does not make up numbers"
    );
    assert_eq!(reading.class, Class::NoApi);
    assert_eq!(
        provider("gemini").variables(),
        &["GOOGLE_API_KEY"],
        "Gemini uses `GOOGLE_API_KEY`, not `GEMINI_API_KEY`"
    );
}

/// Exa: the probe spent a request and got 401 from the admin API
/// (`admin-api.exa.ai`) — the search key cannot read the balance. The provider
/// does not repeat the request: `Unsupported` without network, and the
/// projection would give the same (`unsupported`) had the HTTP status been 200.
#[test]
fn exa_is_unsupported_without_spending_a_request() {
    let provider = provider("exa");
    let empty = Credentials::default();

    assert_eq!(provider.class(), Class::NoApi);
    assert_eq!(
        provider.read(&Http::new(1), &empty),
        Err(Status::Unsupported),
        "with no balance route, the request is not made"
    );
    assert_eq!(
        provider.interpret(&[], Utc::now()),
        Err(Status::Unsupported)
    );

    let expected = reading("exa");
    assert_eq!(expected.status, Status::InvalidCredential { http: 401 });
    assert!(expected.meters.is_empty());
}

// -------------------------------------------------------------- invalid body

/// A body that is not what the provider expected gives `UnexpectedFormat`,
/// never a panic — that is what keeps the TUI alive when an API changes.
#[test]
fn empty_body_is_unexpected_format() {
    let now = cache().generated_at;
    let empty = Response {
        http: 200,
        headers: Vec::new(),
        body: "{}".to_string(),
        duration_ms: 1,
    };

    for id in [
        "tavily",
        "firecrawl",
        "serper",
        "jina",
        "openrouter",
        "elevenlabs",
        "twilio",
        "github",
        "brave",
        "context7",
        "deepseek",
        "minimax",
        "deepgram",
        "fal",
        "pushover",
    ] {
        let result = provider(id).interpret(std::slice::from_ref(&empty), now);
        assert!(
            matches!(result, Err(Status::UnexpectedFormat { .. })),
            "{id}: {result:?}"
        );
    }

    // The four `NoApi` providers validate the body but look for no fields, so
    // a `{}` with HTTP 200 still proves what the request proved: that the key
    // works. There is nothing to measure, and no number is made up.
    for id in ["groq", "gemini", "mistral", "cerebras"] {
        let result = provider(id).interpret(std::slice::from_ref(&empty), now);
        assert_eq!(result, Ok(Vec::new()), "{id}");
    }

    // Claude does not look for fields either: none of the four windows is in
    // the `{}` and none is required (this is `p_claude` in `project.py`, where
    // the account has no Opus window).
    assert_eq!(
        provider("claude").interpret(std::slice::from_ref(&empty), now),
        Ok(Vec::new())
    );

    // xAI, Composio and X API do not even look at the body: what is missing is
    // the unconfirmed format, and that holds for any body.
    for id in ["xai", "composio", "x"] {
        assert_eq!(
            provider(id).interpret(std::slice::from_ref(&empty), now),
            Err(Status::Unsupported),
            "{id}"
        );
    }

    // A body that is not valid JSON, however, proves nothing, even with 200.
    for body in ["", "null", "{", "<html>502 Bad Gateway</html>"] {
        let response = Response {
            http: 200,
            headers: Vec::new(),
            body: body.to_string(),
            duration_ms: 1,
        };
        for provider in all() {
            let result = provider.interpret(std::slice::from_ref(&response), now);
            match provider.id() {
                // With no balance route or no confirmed format, no body is
                // good enough: the status is the same.
                "exa" | "xai" | "composio" | "x" => {
                    assert_eq!(result, Err(Status::Unsupported), "{}", provider.id())
                }
                _ => assert!(
                    matches!(result, Err(Status::UnexpectedFormat { .. })),
                    "{}: body `{body}` → {result:?}",
                    provider.id()
                ),
            }
        }
    }

    // With no responses at all there is nothing to interpret either.
    for provider in all() {
        let result = provider.interpret(&[], now);
        match provider.id() {
            "exa" | "xai" | "composio" | "x" => {
                assert_eq!(result, Err(Status::Unsupported), "{}", provider.id())
            }
            _ => assert!(
                matches!(result, Err(Status::UnexpectedFormat { .. })),
                "{}: {result:?}",
                provider.id()
            ),
        }
    }
}
