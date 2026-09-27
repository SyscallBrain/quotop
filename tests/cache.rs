//! Cache tests: round trip with mode 600, lenient reading (missing, corrupted,
//! unknown version — never deleted) and the anti-leak guard before saving.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use quotop::cache;
use quotop::model::{CACHE_VERSION, Cache, Status};
use quotop::secret::Secret;

/// The real cache measured by the probe, with the 26 readings.
const FIXTURE: &str = include_str!("fixtures/readings_2026-09-26.json");

fn fixture_cache() -> Cache {
    serde_json::from_str(FIXTURE).expect("the fixture is a valid cache")
}

#[test]
fn saves_and_reads_back_with_mode_600() {
    let temp = common::Temp::new("cache-round-trip");
    let target = temp.root().join("quotop/last.json");
    let cache = fixture_cache();

    let text = cache::prepare(&cache, &[]).expect("without secrets there is no leak");
    cache::save(&target, &text).expect("save the cache");

    let mode = fs::metadata(&target)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the cache is born with mode 600");

    let mut warnings = Vec::new();
    let read = cache::read(&target, &[], &mut warnings).expect("the cache can be read");
    assert_eq!(read, cache);
    assert_eq!(read.readings.len(), 26, "the fixture has the 26 readings");
    assert!(warnings.is_empty(), "warnings: {warnings:?}");

    // No temporary file is left behind.
    let leftovers: Vec<String> = fs::read_dir(target.parent().expect("cache directory"))
        .expect("read_dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name != "last.json")
        .collect();
    assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
}

#[test]
fn saving_over_the_previous_one_leaves_no_temporary_files() {
    let temp = common::Temp::new("cache-rewritten");
    let target = temp.root().join("quotop/last.json");
    let cache = fixture_cache();

    for _ in 0..3 {
        let text = cache::prepare(&cache, &[]).expect("prepare");
        cache::save(&target, &text).expect("save");
    }

    let mut warnings = Vec::new();
    assert_eq!(cache::read(&target, &[], &mut warnings), Some(cache));
    let files = fs::read_dir(target.parent().expect("cache directory"))
        .expect("read_dir")
        .count();
    assert_eq!(files, 1, "only the cache, no temporary files");
}

#[test]
fn a_corrupted_cache_is_ignored_and_not_deleted() {
    let temp = common::Temp::new("cache-corrupted");
    let path = temp.file(
        "quotop/last.json",
        "{\"version\": 2, \"readings\": [",
        0o600,
    );
    let before = common::read(&path);

    let mut warnings = Vec::new();
    assert!(
        cache::read(&path, &[], &mut warnings).is_none(),
        "nothing is salvaged"
    );
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("unreadable"),
        "warning: {}",
        warnings[0]
    );
    assert!(path.exists(), "the corrupted cache is not deleted");
    assert_eq!(common::read(&path), before, "and the file is not touched");
}

#[test]
fn a_cache_of_another_version_is_ignored_and_not_deleted() {
    // Version 1 is the old Portuguese schema; `CACHE_VERSION + 1` is a newer
    // one. Both are refused the same way.
    for version in [1, CACHE_VERSION + 1] {
        let temp = common::Temp::new("cache-version");
        let text = format!(
            "{{\"version\": {version}, \"generated_at\": \"2026-09-26T11:53:04+00:00\", \"readings\": []}}"
        );
        let path = temp.file("quotop/last.json", &text, 0o600);

        let mut warnings = Vec::new();
        assert!(
            cache::read(&path, &[], &mut warnings).is_none(),
            "version {version}"
        );
        assert_eq!(warnings.len(), 1, "version {version}");
        assert!(warnings[0].contains("version"), "warning: {}", warnings[0]);
        assert!(path.exists(), "a cache that is not ours is not deleted");
        assert_eq!(common::read(&path), text.as_bytes());
    }
}

#[test]
fn a_version_1_cache_with_portuguese_fields_is_ignored_with_a_warning() {
    // The real shape of an old cache: its fields do not even deserialize into
    // the current `Cache`, and that must be a warning, not a crash.
    let temp = common::Temp::new("cache-v1");
    let text = "{\"versao\": 1, \"gerado_em\": \"2026-09-26T11:53:04+00:00\", \"leituras\": []}";
    let path = temp.file("quotop/last.json", text, 0o600);

    let mut warnings = Vec::new();
    assert!(cache::read(&path, &[], &mut warnings).is_none());
    assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
    assert!(path.exists());
}

#[test]
fn a_missing_cache_is_not_a_warning() {
    let temp = common::Temp::new("cache-missing");
    let path = temp.root().join("quotop/last.json");

    let mut warnings = Vec::new();
    assert!(cache::read(&path, &[], &mut warnings).is_none());
    assert!(
        warnings.is_empty(),
        "the first run is not an error: {warnings:?}"
    );
}

#[test]
fn a_cache_without_read_permission_is_ignored_with_a_warning() {
    let temp = common::Temp::new("cache-no-permission");
    let text = serde_json::to_string(&fixture_cache()).expect("serialize");
    let path = temp.file("quotop/last.json", &text, 0o000);

    // Running as root (some CI) mode 000 does not prevent reading.
    if fs::read_to_string(&path).is_ok() {
        return;
    }

    let mut warnings = Vec::new();
    assert!(cache::read(&path, &[], &mut warnings).is_none());
    assert_eq!(warnings.len(), 1);
    assert!(path.exists());
}

#[test]
fn a_cache_with_a_secret_is_not_written() {
    let temp = common::Temp::new("cache-leak");
    let target = temp.root().join("quotop/last.json");
    let fake = Secret::new("fake-key-for-the-cache-test-0123456789");

    let mut cache = fixture_cache();
    let provider = cache.readings[0].provider.clone();
    cache.readings[0].status = Status::ApiError {
        http: 401,
        message: format!("invalid key: {}", fake.expose()),
    };

    let error = cache::prepare(&cache, &[&fake]).expect_err("the guard must catch this");
    assert_eq!(error.to_string(), format!("leak prevented: {provider}"));
    assert!(
        !error.to_string().contains(fake.expose()),
        "the secret's value never appears in a message: {error}"
    );

    // Without prepared text there is nothing to save — the file does not exist.
    assert!(!target.exists());
    assert!(!fs::read_dir(temp.root()).expect("read_dir").any(|e| {
        e.ok()
            .is_some_and(|e| e.file_name().to_string_lossy().contains("last.json"))
    }));
}

#[test]
fn without_secrets_in_it_the_same_cache_is_written() {
    let temp = common::Temp::new("cache-no-keys");
    let target = temp.root().join("quotop/last.json");
    let cache = fixture_cache();
    let fake = Secret::new("a-key-that-is-not-in-the-cache-0123456789");

    // A secret that is not in the cache blocks nothing.
    let text = cache::prepare(&cache, &[&fake]).expect("no leak");
    cache::save(&target, &text).expect("save");
    let mut warnings = Vec::new();
    assert_eq!(cache::read(&target, &[&fake], &mut warnings), Some(cache));
}
