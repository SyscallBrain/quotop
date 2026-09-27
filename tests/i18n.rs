//! Translation checks. These are what keep a contributed language complete:
//! every locale must have exactly the English keys, with the same
//! `{placeholders}`, and every key the code uses must exist.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use quotop::i18n::{self, LANGUAGES};

fn locale(code: &str) -> HashMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join(format!("{code}.toml"));
    let source = std::fs::read_to_string(&path).expect("read the locale file");
    i18n::parse(&source).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        found.insert(rest[start + 1..start + end].to_string());
        rest = &rest[start + end + 1..];
    }
    found
}

#[test]
fn languages_have_unique_codes_and_english_comes_first() {
    let codes = i18n::codes();
    let unique: BTreeSet<_> = codes.iter().collect();
    assert_eq!(
        unique.len(),
        codes.len(),
        "duplicated language code: {codes:?}"
    );
    assert_eq!(LANGUAGES[i18n::DEFAULT].code, "en-US");
}

#[test]
fn every_language_has_exactly_the_english_keys_and_placeholders() {
    let english = locale("en-US");
    for language in LANGUAGES {
        // `[labels]` translates the English meter labels, so it is empty in
        // English by design and only exists in the other languages.
        let other: HashMap<String, String> = locale(language.code)
            .into_iter()
            .filter(|(key, _)| !key.starts_with("labels."))
            .collect();
        let missing: BTreeSet<_> = english.keys().filter(|k| !other.contains_key(*k)).collect();
        let extra: BTreeSet<_> = other.keys().filter(|k| !english.contains_key(*k)).collect();
        assert!(
            missing.is_empty(),
            "{}: missing keys {missing:?}",
            language.code
        );
        assert!(
            extra.is_empty(),
            "{}: keys not in en-US {extra:?}",
            language.code
        );
        for (key, text) in &english {
            assert_eq!(
                placeholders(text),
                placeholders(&other[key]),
                "{}: placeholders of `{key}` differ from en-US",
                language.code
            );
        }
    }
}

/// Every `t!("…")`, `i18n::text("…")`, `i18n::format("…")` and
/// `i18n::plural("…")` in `src/` names a key that exists in English.
#[test]
fn every_key_used_in_the_source_exists() {
    let english = locale("en-US");
    let mut used = Vec::new();
    let mut plurals = Vec::new();
    for entry in walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
        let source = std::fs::read_to_string(&entry).expect("read source");
        for (marker, is_plural) in [
            ("t!(\"", false),
            ("i18n::text(\"", false),
            ("i18n::format(\"", false),
            ("i18n::plural(\"", true),
        ] {
            let mut rest = source.as_str();
            while let Some(start) = rest.find(marker) {
                let tail = &rest[start + marker.len()..];
                let end = tail.find('"').expect("closing quote");
                // `format!("…")` also ends in `t!("`: only a whole word counts.
                let whole_word = !rest[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                if whole_word {
                    let found = (entry.display().to_string(), tail[..end].to_string());
                    if is_plural {
                        plurals.push(found)
                    } else {
                        used.push(found)
                    }
                }
                rest = &tail[end..];
            }
        }
    }
    assert!(!used.is_empty(), "the scan found no keys at all");
    for (file, key) in used {
        assert!(
            english.contains_key(&key),
            "{file}: key `{key}` is not in en-US.toml"
        );
    }
    for (file, key) in plurals {
        for form in ["one", "other"] {
            assert!(
                english.contains_key(&format!("{key}.{form}")),
                "{file}: plural key `{key}.{form}` is not in en-US.toml"
            );
        }
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn lookup_falls_back_to_english_then_to_the_key() {
    i18n::set_current(i18n::index_of("pt-PT").expect("pt-PT exists"));
    assert_eq!(i18n::decimal_separator(), ',');
    assert_eq!(i18n::text("no.such.key"), "no.such.key");
    i18n::set_current(i18n::DEFAULT);
    assert_eq!(i18n::decimal_separator(), '.');
}

#[test]
fn language_codes_are_matched_loosely_and_resolved_in_order() {
    assert_eq!(i18n::index_of("pt_pt"), i18n::index_of("pt-PT"));
    assert_eq!(i18n::index_of("EN-us"), Some(i18n::DEFAULT));
    assert_eq!(i18n::index_of("xx-XX"), None);

    let pt = i18n::index_of("pt-PT").expect("pt-PT exists");
    let mut warnings = Vec::new();
    assert_eq!(i18n::resolve(None, None, &mut warnings), i18n::DEFAULT);
    assert_eq!(i18n::resolve(Some("pt-PT"), None, &mut warnings), pt);
    // The saved choice wins over config.toml.
    assert_eq!(
        i18n::resolve(Some("pt-PT"), Some("en-US"), &mut warnings),
        i18n::DEFAULT
    );
    assert!(warnings.is_empty());
    assert_eq!(
        i18n::resolve(Some("klingon"), None, &mut warnings),
        i18n::DEFAULT
    );
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("klingon"), "{warnings:?}");
}
