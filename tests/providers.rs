//! The pieces shared by the providers: the `Debug` of [`Request`], the
//! authentication headers and the JSON readers.
//!
//! The values used here are made up: no real key goes through this file.

use chrono::{TimeZone, Utc};
use quotop::model::{Level, Status, Unit};
use quotop::providers::{
    MeterBuilder, Request, Response, bad_format, basic, bearer, body_of, header,
    instant_from_epoch, instant_from_iso, instant_from_text, number, number_at, text_at,
};
use quotop::secret::{MARKER, Secret};
use serde_json::json;

const NO_SECRET: &str = "MUST-NOT-APPEAR-1234567890";

/// A request URL may contain the key in the *query string* (Jina) and the
/// headers carry secrets: an accidental `{:?}` must not spit out either.
#[test]
fn request_debug_shows_neither_url_nor_values() {
    let request = Request::get(
        "https://example.test/api?api_key=MUST-NOT-APPEAR-1234567890",
        bearer(&Secret::new(NO_SECRET)),
    );

    let text = format!("{request:?}");

    assert!(!text.contains(NO_SECRET), "the secret leaked: {text}");
    assert!(!text.contains("api_key"), "the URL leaked: {text}");
    assert!(
        text.contains("Authorization"),
        "the header name is useful: {text}"
    );
    assert!(text.contains("Get"), "{text}");
}

#[test]
fn bearer_and_basic_build_the_right_header() {
    let headers = bearer(&Secret::new("a-fake-key"));
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0, "Authorization");
    assert_eq!(headers[0].1.expose(), "Bearer a-fake-key");

    // `basic` is standard base64 (`RFC 4648`), as Twilio requires. Both sides
    // of the comparison come from Python's `base64.b64encode`, for the three
    // possible remainders of `chunks(3)`: 0, 1 and 2 bytes.
    for (pair, expected) in [
        ("a:b", "Basic YTpi"),
        ("ab:cd", "Basic YWI6Y2Q="),
        ("user:pass", "Basic dXNlcjpwYXNz"),
        (
            "AC1234567890abcdef1234567890abcd:falsatoken",
            "Basic QUMxMjM0NTY3ODkwYWJjZGVmMTIzNDU2Nzg5MGFiY2Q6ZmFsc2F0b2tlbg==",
        ),
    ] {
        let (user, password) = pair.split_once(':').expect("the pair has two fields");
        let headers = basic(&Secret::new(user), &Secret::new(password));
        assert_eq!(headers[0].1.expose(), expected, "pair `{pair}`");
    }

    let headers = header("x-goog-api-key", String::from("another-fake-key"));
    assert_eq!(headers[0].0, "x-goog-api-key");
    assert_eq!(headers[0].1.expose(), "another-fake-key");
}

#[test]
fn number_accepts_decimal_string_and_rejects_the_rest() {
    // Twilio returns the balance quoted.
    assert_eq!(number(&json!("8.1257")), Some(8.1257));
    assert_eq!(number(&json!(" 1.5 ")), Some(1.5));
    assert_eq!(number(&json!(42)), Some(42.0));
    assert_eq!(number(&json!("not a number")), None);
    assert_eq!(number(&json!(null)), None);
    assert_eq!(number(&json!(["1"])), None);

    let body = json!({ "balance": "8.1257", "currency": "USD" });
    assert_eq!(number_at(&body, "balance"), Ok(8.1257));
    assert_eq!(text_at(&body, "currency"), Ok("USD".to_string()));

    // `balance` is text and so is `currency`: each reader accepts what is its
    // own.
    assert_eq!(text_at(&body, "balance"), Ok("8.1257".to_string()));

    // Missing field or wrong type: `UnexpectedFormat`, with a message of our
    // own (the name of the field we expected, not the response text).
    assert!(matches!(
        number_at(&body, "amount"),
        Err(Status::UnexpectedFormat { .. })
    ));
    assert!(matches!(
        number_at(&body, "currency"),
        Err(Status::UnexpectedFormat { .. })
    ));
    let numeric = json!({ "n": 1 });
    assert!(matches!(
        text_at(&numeric, "n"),
        Err(Status::UnexpectedFormat { .. })
    ));
}

#[test]
fn instants_are_read_to_the_second() {
    // `iso_norm` in `project.py` (which made the fixture) drops the fraction of
    // a second: the comparison with the fixture only matches if we drop it
    // too.
    let body = json!({ "end": "2026-10-08T12:18:25.660Z" });
    assert_eq!(
        instant_from_iso(&body, "end"),
        Ok(Utc.with_ymd_and_hms(2026, 10, 8, 12, 18, 25).unwrap())
    );

    assert_eq!(
        instant_from_text("2026-09-26T11:53:04+00:00"),
        Some(Utc.with_ymd_and_hms(2026, 9, 26, 11, 53, 4).unwrap())
    );
    assert_eq!(instant_from_text("2026-09-26"), None);

    // The fixture's `unix_iso`: 1790427183 → 2026-09-26T12:53:03Z.
    assert_eq!(
        instant_from_epoch(1_790_427_183),
        Some(Utc.with_ymd_and_hms(2026, 9, 26, 12, 53, 3).unwrap())
    );
    assert_eq!(instant_from_epoch(i64::MAX), None);

    let body = json!({ "reset": null });
    assert!(matches!(
        instant_from_iso(&body, "reset"),
        Err(Status::UnexpectedFormat { .. })
    ));
}

#[test]
fn body_of_reads_the_body_and_rejects_garbage() {
    let response = |body: &str| Response {
        http: 200,
        headers: Vec::new(),
        body: body.to_string(),
        duration_ms: 1,
    };

    assert_eq!(body_of(&[response("{\"a\": 1}")]), Ok(json!({ "a": 1 })));
    assert!(matches!(
        body_of(&[response("<html>502</html>")]),
        Err(Status::UnexpectedFormat { .. })
    ));
    assert!(matches!(body_of(&[]), Err(Status::UnexpectedFormat { .. })));
}

#[test]
fn format_messages_carry_our_own_field_name() {
    assert_eq!(
        bad_format("the response has no `balance`"),
        Status::UnexpectedFormat {
            message: "the response has no `balance`".to_string()
        }
    );
}

/// `MeterBuilder` does the arithmetic: `remaining` derived, rounded to 6
/// decimal places, and the level coming from `model::level`.
#[test]
fn meter_derives_remaining_and_level() {
    let meter = MeterBuilder::new("monthly plan", Unit::Credits)
        .used(1234.5)
        .limit(1000.0)
        .build();
    assert_eq!(meter.remaining, Some(-234.5));
    assert_eq!(meter.level, Level::Exhausted);

    // Without `used` or `limit`, the remainder decides the level.
    let loose = MeterBuilder::new("credits", Unit::Credits)
        .remaining(0.0)
        .build();
    assert_eq!(loose.remaining, Some(0.0));
    assert_eq!(loose.level, Level::Exhausted);

    // An explicit `remaining` is never replaced by the computed one.
    let explicit = MeterBuilder::new("credits", Unit::Credits)
        .used(10.0)
        .limit(100.0)
        .remaining(1394.0)
        .build();
    assert_eq!(explicit.remaining, Some(1394.0));

    // `Debug` of a meter with no secret inside: it shows the numbers.
    assert!(format!("{meter:?}").contains("1234.5"));
}

/// A `Secret` inside a `Meter`/`Status` (which go to the cache and to the
/// screen) is impossible by construction: `Secret` is not `Serialize` and its
/// `Debug` hides it.
#[test]
fn secret_hides_the_value_in_debug_and_display() {
    let secret = Secret::new(NO_SECRET);
    assert_eq!(format!("{secret:?}"), MARKER);
    assert_eq!(format!("{secret}"), MARKER);
}
