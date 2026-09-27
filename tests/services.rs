//! Tests of the Services & keys screen's logic, off screen: the visibility
//! rule, the file where it is saved, and writing keys to `keys.env`.

mod common;

use std::os::unix::fs::PermissionsExt;

use quotop::credentials::{self, Source};
use quotop::keyfile;
use quotop::secret::Secret;
use quotop::tui::visibility::{self, Visibility};

#[test]
fn by_default_services_with_a_credential_appear() {
    let v = Visibility::default();
    assert!(v.is_visible("tavily", true));
    assert!(!v.is_visible("deepseek", false));
}

#[test]
fn toggling_stores_only_the_exceptions() {
    let mut v = Visibility::default();

    // Hiding one with a key is an exception; showing it again removes it.
    v.toggle("tavily", true);
    assert!(!v.is_visible("tavily", true));
    assert!(v.hide.contains("tavily"));
    v.toggle("tavily", true);
    assert!(v.is_visible("tavily", true));
    assert_eq!(v, Visibility::default(), "back to the rule, no exceptions");

    // Showing one without a key is also an exception.
    v.toggle("deepseek", false);
    assert!(v.is_visible("deepseek", false));
    assert!(v.show.contains("deepseek"));

    // `show` does not hide what already appears.
    v.show("deepseek", false);
    assert!(v.is_visible("deepseek", false));
    v.show("groq", true);
    assert!(v.is_visible("groq", true));
    assert!(
        !v.show.contains("groq"),
        "it already appeared by the rule: no exception"
    );
}

#[test]
fn visibility_saves_and_reads_back() {
    let temp = common::Temp::new("services-save");
    let path = temp.root().join("state").join(visibility::FILE_NAME);
    assert_eq!(visibility::read(&path), None);

    let mut v = Visibility::default();
    v.toggle("deepseek", false);
    v.toggle("exa", true);
    visibility::save(&path, &v).expect("save");
    assert_eq!(visibility::read(&path), Some(v));
    assert!(!path.with_extension("toml.tmp").exists());
}

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn saving_a_key_to_a_new_file() {
    let temp = common::Temp::new("keys-new");
    let path = temp.root().join("quotop").join("keys.env");
    let value = Secret::new("sk-test-new-123");
    keyfile::save(&path, &[("DEEPSEEK_API_KEY", &value)]).expect("save");

    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "DEEPSEEK_API_KEY=sk-test-new-123\n"
    );
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().expect("directory")), 0o700);
    assert!(!path.with_extension("env.tmp").exists());

    // The usual reader reads what was saved.
    let source = Source {
        env: Vec::new(),
        key_files: vec![path],
        home_dir: Some(temp.root().to_path_buf()),
    };
    let cred = credentials::load(&source, &mut Vec::new());
    assert_eq!(
        cred.value("DEEPSEEK_API_KEY").map(Secret::expose),
        Some("sk-test-new-123")
    );
}

#[test]
fn saving_changes_only_the_variable_line() {
    let temp = common::Temp::new("keys-existing");
    let before = "# my keys\nTAVILY_API_KEY=tvly-old\n\nexport DEEPSEEK_API_KEY=\"sk-stale\"\nJINA_API_KEY=jina # with a hash\n";
    // A file with loose permissions is rewritten with 600 (and becomes
    // readable).
    let path = temp.file("keys.env", before, 0o644);

    let deepseek = Secret::new("sk-new");
    let fal = Secret::new("fal-new");
    keyfile::save(&path, &[("DEEPSEEK_API_KEY", &deepseek), ("FAL_KEY", &fal)]).expect("save");

    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "# my keys\nTAVILY_API_KEY=tvly-old\n\nDEEPSEEK_API_KEY=sk-new\nJINA_API_KEY=jina # with a hash\nFAL_KEY=fal-new\n"
    );
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn values_that_do_not_fit_on_one_line_are_rejected() {
    let temp = common::Temp::new("keys-invalid");
    let path = temp.root().join("keys.env");
    for bad in ["", "with space", "two\nlines", "tab\tinside"] {
        let value = Secret::new(bad);
        let error = keyfile::save(&path, &[("X_API_KEY", &value)]).expect_err(bad);
        // The message never carries the value.
        let text = error.to_string();
        if !bad.is_empty() {
            assert!(!text.contains(bad), "{text}");
        }
    }
    assert!(!path.exists(), "nothing was written");
}
