//! The anti-leak barrier and the `http → Status` rule.
//!
//! The values used here are made up for the tests: no real key goes through
//! this file.

use quotop::guard::{LIMIT, MIN_TO_SEARCH, check_no_leak, mask, middle_chunk};
use quotop::model::Status;
use quotop::providers::status_from_http;
use quotop::secret::{MARKER, Secret};

/// A fake key, long enough to have a middle chunk (> 16).
const KEY: &str = "sk-fake-0123456789abcdefghij-TAIL-zzz";

#[test]
fn masks_the_whole_secret() {
    let key = Secret::new(KEY);
    let text = format!("401 on account {KEY}: invalid key");

    let output = mask(&text, &[&key]);

    assert!(output.contains(MARKER));
    assert!(!output.contains(KEY), "the whole key got out: {output}");
}

#[test]
fn masks_the_middle_chunk_of_a_long_secret() {
    let key = Secret::new(KEY);
    let chunk = middle_chunk(KEY).expect("the test key has a middle chunk");
    assert_eq!(chunk.chars().count(), 12);

    // An API error that returns only a piece of the key (truncated message,
    // server *log*) must not get through.
    let output = mask(&format!("key «{chunk}» rejected"), &[&key]);

    assert!(output.contains(MARKER));
    assert!(!output.contains(&chunk), "the chunk got out: {output}");
}

#[test]
fn a_short_secret_is_ignored_but_one_at_the_minimum_is_masked_whole() {
    // Below `MIN_TO_SEARCH` the value is neither searched for nor masked: a
    // `SOME_TOOL_DEBUG=0` in a shared `.env` must not bring down `--json` by
    // matching a `"duration_ms": 0`.
    let short = Secret::new("abc123");
    assert_eq!(middle_chunk("abc123"), None);
    assert_eq!(
        mask("unknown token abc123", &[&short]),
        "unknown token abc123",
        "below {MIN_TO_SEARCH} characters the value is ignored"
    );

    // At the minimum (8) it is searched for, and the whole value is masked.
    let minimum = Secret::new("abc12345");
    assert_eq!(middle_chunk("abc12345"), None, "too short to have a chunk");
    assert_eq!(
        mask("unknown token abc12345", &[&minimum]),
        format!("unknown token {MARKER}")
    );
}

#[test]
fn a_trivial_value_does_not_trip_the_guard_and_eight_characters_do() {
    assert_eq!(MIN_TO_SEARCH, 8);

    // A `SOME_TOOL_DEBUG=0` would become `Secret("0")` and match the
    // `"duration_ms": 0` of any reading.
    let zero = Secret::new("0");
    assert_eq!(
        check_no_leak("{\"duration_ms\": 0}", &[&zero]),
        Ok(()),
        "a 1-character value is not searched for"
    );
    assert_eq!(
        mask("{\"duration_ms\": 0}", &[&zero]),
        "{\"duration_ms\": 0}"
    );

    // Seven characters are still too few; eight are already searched for.
    let seven = Secret::new("FAKE__7");
    assert_eq!(check_no_leak("a FAKE__7 b", &[&seven]), Ok(()));
    assert_eq!(mask("a FAKE__7 b", &[&seven]), "a FAKE__7 b");

    let eight = Secret::new("FAKE_008");
    assert!(check_no_leak("a FAKE_008 b", &[&eight]).is_err());
    assert!(mask("a FAKE_008 b", &[&eight]).contains(MARKER));
}

#[test]
fn strips_control_characters_and_cuts_at_120() {
    // A `\u{1b}` in an API message paints the terminal; a 300-character body
    // fills the screen.
    let text = format!("\u{1b}[31merror\u{7}\n{}", "x".repeat(300));

    let output = mask(&text, &[]);

    assert!(
        !output.chars().any(char::is_control),
        "control characters left: {output:?}"
    );
    assert_eq!(output.chars().count(), LIMIT);
    assert!(
        output.starts_with("[31merror"),
        "the useful text was lost: {output}"
    );
}

#[test]
fn check_no_leak_catches_whole_and_chunk() {
    let key = Secret::new(KEY);
    let chunk = middle_chunk(KEY).expect("middle chunk");

    assert_eq!(check_no_leak("{\"status\":\"ok\"}", &[&key]), Ok(()));
    assert!(check_no_leak(&format!("{{\"url\":\"https://x/?k={KEY}\"}}"), &[&key]).is_err());
    assert!(check_no_leak(&format!("…{chunk}…"), &[&key]).is_err());

    // An empty secret does not make the guard fire on everything.
    assert_eq!(
        check_no_leak("any text at all", &[&Secret::new("")]),
        Ok(())
    );
}

#[test]
fn status_from_http_follows_the_contract() {
    // `None` = usable response; everything else is an error `Status`.
    assert_eq!(status_from_http(200), None);
    assert_eq!(status_from_http(204), None);
    assert_eq!(
        status_from_http(401),
        Some(Status::InvalidCredential { http: 401 })
    );
    assert_eq!(
        status_from_http(403),
        Some(Status::InvalidCredential { http: 403 })
    );
    assert_eq!(
        status_from_http(429),
        Some(Status::RateLimited { http: 429 })
    );
    assert_eq!(
        status_from_http(500),
        Some(Status::ApiError {
            http: 500,
            message: String::new()
        })
    );
    assert_eq!(
        status_from_http(400),
        Some(Status::ApiError {
            http: 400,
            message: String::new()
        })
    );
}
