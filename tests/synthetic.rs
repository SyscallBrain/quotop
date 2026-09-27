//! The bodies the 2026-09-26 probe did not measure — and the expired Claude.
//!
//! DeepSeek, Deepgram, fal.ai and Pushover got no real body in that
//! measurement (the probe did not have the credential); their expected body
//! comes from the **documentation** and lives in `tests/fixtures/synthetic/`.
//! These tests prove that `interpret` reads that body the way the
//! documentation describes it — they are worth as much as the documentation is.
//! When the real body arrives, the fixture changes; the provider only changes
//! if the documentation was wrong.
//!
//! MiniMax is the odd one out: the probe brought back a **real** body (HTTP 200
//! with a business error inside) that must be treated as an error — that test
//! reads the actual probe, not a synthetic fixture.
//!
//! No test here touches the network: `interpret` is pure, and the expired
//! Claude's `read` refuses before building the request.

mod common;

use chrono::{DateTime, Utc};
use serde_json::Value;

use quotop::config::Config;
use quotop::credentials::{self, Credentials};
use quotop::http::Http;
use quotop::model::{Level, Meter, Status, Unit};
use quotop::providers::{Claude, Provider, Response, all};

use common::Temp;

const DEEPSEEK: &str = include_str!("fixtures/synthetic/deepseek_balance.json");
const DEEPGRAM_PROJECTS: &str = include_str!("fixtures/synthetic/deepgram_projects.json");
const DEEPGRAM_BALANCES: &str = include_str!("fixtures/synthetic/deepgram_balances.json");
const FAL: &str = include_str!("fixtures/synthetic/fal_billing.json");
const PUSHOVER: &str = include_str!("fixtures/synthetic/pushover_limits.json");
const MINIMAX: &str = include_str!("fixtures/synthetic/minimax_token_plan.json");
const PROBE: &str = include_str!("fixtures/probe_2026-09-26.json");

/// The instant of the checks; only Claude and Pushover depend on it.
fn now() -> DateTime<Utc> {
    "2026-09-26T11:53:04Z".parse().expect("valid instant")
}

fn provider(id: &str) -> Box<dyn Provider> {
    all()
        .into_iter()
        .find(|p| p.id() == id)
        .unwrap_or_else(|| panic!("provider `{id}` is not in all()"))
}

/// A `200` response with the given body (empty headers: none of these
/// providers takes numbers from headers).
fn response(body: &str) -> Response {
    Response {
        http: 200,
        headers: Vec::new(),
        body: body.to_string(),
        duration_ms: 1,
    }
}

/// The only meter this body produces.
fn single_meter(provider_id: &str, body: &str) -> Meter {
    let meters = provider(provider_id)
        .interpret(&[response(body)], now())
        .unwrap_or_else(|e| panic!("{provider_id}: {e:?}"));
    assert_eq!(meters.len(), 1, "{provider_id}");
    meters.into_iter().next().expect("there is one meter")
}

/// The probe response with this id, as `http.rs` would deliver it.
fn probe_response(probe_id: &str) -> Response {
    let run: Value = serde_json::from_str(PROBE).expect("the probe is JSON");
    let probe = run["probes"]
        .as_array()
        .expect("`probes` is a list")
        .iter()
        .find(|p| p["id"] == probe_id)
        .unwrap_or_else(|| panic!("the fixture has no probe `{probe_id}`"));
    let r = &probe["response"];
    assert_eq!(r["http"], 200, "probe `{probe_id}` had to be a 200");
    Response {
        http: 200,
        headers: Vec::new(),
        body: serde_json::to_string(&r["body"]).expect("the probe body is JSON"),
        duration_ms: r["ms"].as_u64().unwrap_or(0),
    }
}

#[test]
fn synthetic_fixtures_state_their_origin() {
    // Every synthetic fixture must declare that it was not measured: without
    // this, a made-up fixture would pass for a real body and nobody would know.
    for (name, text) in [
        ("deepseek_balance.json", DEEPSEEK),
        ("deepgram_projects.json", DEEPGRAM_PROJECTS),
        ("deepgram_balances.json", DEEPGRAM_BALANCES),
        ("fal_billing.json", FAL),
        ("pushover_limits.json", PUSHOVER),
        ("minimax_token_plan.json", MINIMAX),
    ] {
        let json: Value = serde_json::from_str(text).expect("the fixture is JSON");
        let origin = json["_origin"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: missing `_origin` field"));
        assert!(
            origin.starts_with("synthetic, from the documentation"),
            "{name}: {origin}"
        );
        assert!(
            origin.contains("https://"),
            "{name}: missing URL in {origin}"
        );
        assert!(
            origin.ends_with("2026-09-26"),
            "{name}: missing date in {origin}"
        );
    }
}

#[test]
fn deepseek_reads_currency_and_balance() {
    let meter = single_meter("deepseek", DEEPSEEK);

    assert_eq!(meter.label, "balance");
    assert_eq!(meter.unit, Unit::Currency);
    assert_eq!(meter.currency.as_deref(), Some("CNY"));
    // The documentation gives the balance as a **decimal string**, not a
    // number.
    assert_eq!(meter.remaining, Some(42.5));
    assert_eq!(meter.used, None);
    assert_eq!(meter.limit, None);
    assert_eq!(meter.resets_at, None);
    // No known limit and no absolute thresholds: undeterminable, not "full".
    assert_eq!(meter.level, Level::NoReference);
}

#[test]
fn deepseek_without_balance_list_makes_up_no_meter() {
    // The documentation guarantees `balance_infos`; without it there is no
    // usable response.
    let result = provider("deepseek").interpret(&[response("{}")], now());
    assert!(matches!(result, Err(Status::UnexpectedFormat { .. })));
}

#[test]
fn deepgram_reads_the_second_response() {
    // Deepgram is the only one that makes two requests: the project list and,
    // with its `project_id`, the balances. `interpret` reads the second one.
    let both = [response(DEEPGRAM_PROJECTS), response(DEEPGRAM_BALANCES)];
    let meters = provider("deepgram")
        .interpret(&both, now())
        .expect("the documented body is readable");
    assert_eq!(meters.len(), 1);

    let meter = &meters[0];
    assert_eq!(meter.label, "balance");
    assert_eq!(meter.unit, Unit::Currency);
    assert_eq!(meter.currency.as_deref(), Some("usd"));
    assert_eq!(meter.remaining, Some(12.3456));
    assert_eq!(meter.level, Level::NoReference);
}

#[test]
fn deepgram_without_the_second_response_is_an_error() {
    let only_one = [response(DEEPGRAM_PROJECTS)];
    let result = provider("deepgram").interpret(&only_one, now());
    assert!(
        matches!(result, Err(Status::UnexpectedFormat { .. })),
        "{result:?}"
    );
}

#[test]
fn fal_reads_the_credits() {
    let meter = single_meter("fal", FAL);

    assert_eq!(meter.label, "credits");
    assert_eq!(meter.unit, Unit::Currency);
    assert_eq!(meter.currency.as_deref(), Some("USD"));
    assert_eq!(meter.remaining, Some(7.42));
    assert_eq!(meter.level, Level::NoReference);
}

#[test]
fn pushover_reads_the_monthly_quota() {
    let meter = single_meter("pushover", PUSHOVER);

    assert_eq!(meter.label, "messages/month");
    assert_eq!(meter.unit, Unit::Requests);
    assert_eq!(meter.limit, Some(10000.0));
    assert_eq!(meter.remaining, Some(9993.0));
    assert_eq!(meter.used, None);
    // The `reset` is an epoch (unlike Brave's relative `reset`).
    assert_eq!(
        meter.resets_at,
        Some("2026-10-01T00:00:00Z".parse().expect("valid instant"))
    );
    // 9993/10000 = 0.9993: well above the default thresholds.
    assert_eq!(meter.level, Level::Ok);
}

#[test]
fn minimax_refusal_inside_a_200() {
    // The probe's real body: HTTP 200 with `base_resp.status_code = 1004`
    // ("login fail"). It is a business error, not a success without numbers.
    let refusal = provider("minimax").interpret(&[probe_response("minimax_token_plan")], now());
    match refusal {
        Err(Status::ApiError { http, .. }) => assert_eq!(http, 200),
        other => panic!("expected `ApiError` with 200, got {other:?}"),
    }

    // Without the business error the body is accepted — but there are no
    // numbers to show: the user's key is pay-as-you-go and the endpoint only
    // serves the Token Plan. It stays `Unsupported`, which is different from
    // "no balance".
    let success = provider("minimax").interpret(&[response(MINIMAX)], now());
    assert_eq!(success, Err(Status::Unsupported));
}

#[test]
fn expired_claude_refuses_without_network() {
    let temp = Temp::new("claude-expired");
    temp.file(
        ".claude/.credentials.json",
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-test-token","expiresAt":1,"refreshToken":"never-used"}}"#,
        0o600,
    );

    let mut warnings = Vec::new();
    let cred = credentials::load(&temp.source(&[], &[]), &mut warnings);

    let claude = cred.claude().expect("the test file has a credential");
    assert!(claude.is_expired(now()), "expiresAt=1ms is in the past");
    assert!(
        warnings.is_empty(),
        "a well-formed file produces no warnings: {warnings:?}"
    );

    // The refusal is decided before there is a request: if `read` touched the
    // network, this value would be different.
    assert_eq!(
        Claude::new().read(&Http::new(1), &cred),
        Err(Status::ExpiredCredential)
    );
}

#[test]
fn claude_without_credential_is_no_credential() {
    assert_eq!(
        Claude::new().read(&Http::new(1), &Credentials::default()),
        Err(Status::NoCredential)
    );
}

#[test]
fn claude_declares_the_file_as_variable() {
    // Claude is the only row in the registry whose "variable" is the path of
    // the file the credential comes from.
    assert_eq!(
        Claude::new().variables(),
        &["~/.claude/.credentials.json"],
        "the detail panel shows this path"
    );
}

#[test]
fn claude_default_user_agent_is_the_config_one() {
    // The provider's default and the `config.toml` default are the same: if
    // someone changes one, this test says the other was left behind.
    assert_eq!(
        Claude::new().user_agent(),
        Config::default().claude_user_agent
    );
    // And the `User-Agent` can be swapped, so the engine can honor the file.
    assert_eq!(
        Claude::with_user_agent("claude-code/9.9.9").user_agent(),
        "claude-code/9.9.9"
    );
}
