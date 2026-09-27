//! Tests of the domain contract against the real fixture of 2026-09-26.

use chrono::{DateTime, Utc};
use quotop::model::{
    CACHE_VERSION, Cache, DEFAULT_CRITICAL, DEFAULT_WARNING, Level, Thresholds, level,
};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} is unreadable: {e}", path.display()))
}

/// (a) The readings fixture deserializes into the `Cache`.
#[test]
fn readings_fixture_deserializes_into_the_cache() {
    let cache: Cache =
        serde_json::from_str(&fixture("readings_2026-09-26.json")).expect("valid Cache");
    assert_eq!(cache.version, CACHE_VERSION);
    assert_eq!(
        cache.readings.len(),
        26,
        "the fixture has 26 readings (one per service, Exa included)"
    );
    assert_eq!(
        cache.generated_at,
        "2026-09-26T11:53:04+00:00"
            .parse::<DateTime<Utc>>()
            .expect("date")
    );
    let ids: Vec<&str> = cache.readings.iter().map(|l| l.provider.as_str()).collect();
    let unique: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "unique provider ids");
}

/// (b) Serializing and reading back gives the same value.
#[test]
fn cache_round_trips() {
    let first: Cache =
        serde_json::from_str(&fixture("readings_2026-09-26.json")).expect("valid Cache");
    let text = serde_json::to_string(&first).expect("serializable");
    let second: Cache = serde_json::from_str(&text).expect("read back");
    assert_eq!(first, second);
}

/// `version` comes first and is the current cache version.
#[test]
fn serialized_cache_starts_with_the_current_version() {
    let cache: Cache =
        serde_json::from_str(&fixture("readings_2026-09-26.json")).expect("valid Cache");
    let text = serde_json::to_string(&cache).expect("serializable");
    let expected = format!(r#"{{"version":{CACHE_VERSION},"generated_at":"#);
    assert!(text.starts_with(&expected), "starts with {text:.40}");
    assert_eq!(CACHE_VERSION, 2, "version 2 is the English schema");
}

/// `Status` is tagged by `kind`, in `snake_case`.
#[test]
fn status_is_tagged_by_kind() {
    let cache: Cache =
        serde_json::from_str(&fixture("readings_2026-09-26.json")).expect("valid Cache");
    let texts: Vec<String> = cache
        .readings
        .iter()
        .map(|l| serde_json::to_string(&l.status).expect("serializable"))
        .collect();
    assert!(texts.iter().any(|t| t == r#"{"kind":"ok"}"#), "{texts:?}");
    assert!(
        texts.iter().any(|t| t == r#"{"kind":"no_credential"}"#),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t == r#"{"kind":"invalid_credential","http":401}"#),
        "{texts:?}"
    );
}

/// (c) The `Level` rule, including the exact boundaries.
#[test]
fn level_follows_the_rules() {
    let default = Thresholds::default();
    assert_eq!(
        default,
        Thresholds::Fraction {
            warning: 0.20,
            critical: 0.05
        }
    );

    // OpenRouter in the fixture: 29.569193949 used of a 30 limit → 0.43 USD
    // left, 1.4 % of the limit, so critical (the only critical one in the real
    // probe).
    assert_eq!(
        level(Some(29.569_193_949), Some(30.0), None, default),
        Level::Critical
    );
    assert_eq!(
        level(None, Some(30.0), Some(0.430_806), default),
        Level::Critical
    );

    // Firecrawl: 1 394 left for a 1 000 plan → `f > 1` gives Ok.
    assert_eq!(level(None, Some(1000.0), Some(1394.0), default), Level::Ok);

    // No limit: 0 is exhausted, 0.5 has no reference.
    assert_eq!(level(None, None, Some(0.0), default), Level::Exhausted);
    assert_eq!(level(None, None, Some(-1.0), default), Level::Exhausted);
    assert_eq!(level(None, None, Some(0.5), default), Level::NoReference);

    // Exact boundaries of the remaining fraction: 0.05 → critical, 0.20 → warning.
    assert_eq!(
        level(None, Some(100.0), Some(5.0), default),
        Level::Critical
    );
    assert_eq!(
        level(None, Some(100.0), Some(20.0), default),
        Level::Warning
    );
    assert_eq!(level(None, Some(100.0), Some(20.001), default), Level::Ok);
    assert_eq!(
        level(None, Some(100.0), Some(0.0), default),
        Level::Exhausted
    );

    // No `remaining` but `used` and `limit`: limit - used is computed.
    assert_eq!(
        level(Some(95.0), Some(100.0), None, default),
        Level::Critical
    );
    // Neither remaining nor used: no level is made up.
    assert_eq!(level(None, Some(100.0), None, default), Level::NoReference);
    // A zero limit counts as "no known limit".
    assert_eq!(level(Some(0.0), Some(0.0), None, default), Level::Exhausted);
    assert_eq!(
        level(Some(0.0), Some(0.0), Some(7.0), default),
        Level::NoReference
    );

    // Absolute thresholds: they only apply to meters with no limit.
    let absolute = Thresholds::Absolute {
        warning: 5.0,
        critical: 1.0,
    };
    assert_eq!(level(None, None, Some(0.5), absolute), Level::Critical);
    assert_eq!(level(None, None, Some(3.0), absolute), Level::Warning);
    assert_eq!(level(None, None, Some(9.0), absolute), Level::Ok);
    assert_eq!(level(None, None, None, absolute), Level::NoReference);

    assert_eq!(DEFAULT_WARNING, 0.20);
    assert_eq!(DEFAULT_CRITICAL, 0.05);
}
