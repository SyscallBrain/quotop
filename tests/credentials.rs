//! Tests of the credentials and of `Secret`.
//!
//! The values used here are fake and recognizable; no test prints a real
//! value, and the tests that compare a value compare it into a boolean, so
//! that a failure does not dump it to the output.

mod common;

use chrono::{DateTime, Utc};

use quotop::credentials::{
    self, VAR_OPENCODE_GO, expand_tilde, looks_like_credential, parse_dotenv,
};
use quotop::secret::{MARKER, Secret};

/// A value that exists in the Claude file but that the app must not keep.
const REFRESH_DO_NOT_USE: &str = "TEST-REFRESH-must-not-be-stored";

const CLAUDE_CREDENTIALS: &str = r#"{
  "claudeAiOauth": {
    "accessToken": "TEST-TOKEN-for-claude",
    "refreshToken": "TEST-REFRESH-must-not-be-stored",
    "expiresAt": 1790000000000,
    "scopes": ["user:inference"],
    "subscriptionType": "max"
  },
  "somethingElse": { "accessToken": "NOT-TO-BE-READ" }
}"#;

fn ms(millis: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(millis).expect("valid instant")
}

#[test]
fn dotenv_parser_reads_the_usual_dotenv_format() {
    let text = "\
# a whole-line comment\n\
\n\
TAVILY_API_KEY=tvly-fake\n\
   export JINA_API_KEY='jina-fake'\n\
OPENROUTER_API_KEY=\"or-fake\"\n\
   \n\
a line without an equals sign\n\
9DOES_NOT_START_WITH_A_LETTER=x\n\
WITH_HASH=abc#this-is-not-a-comment\n";
    let pairs = parse_dotenv(text);
    let names: Vec<&str> = pairs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "TAVILY_API_KEY",
            "JINA_API_KEY",
            "OPENROUTER_API_KEY",
            "WITH_HASH"
        ]
    );
    let value = |name: &str| {
        pairs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    assert!(value("TAVILY_API_KEY") == "tvly-fake");
    assert!(
        value("JINA_API_KEY") == "jina-fake",
        "single quotes and `export`"
    );
    assert!(value("OPENROUTER_API_KEY") == "or-fake", "double quotes");
    assert!(
        value("WITH_HASH") == "abc#this-is-not-a-comment",
        "a `#` inside the value is not a comment"
    );
}

#[test]
fn precedence_is_environment_then_first_file_then_second_file() {
    let temp = common::Temp::new("cred-precedence");
    let first = temp.file(
        "keys.env",
        "PRECEDENCE_API_KEY=from-first-file\nONLY_HERE_API_KEY=from-first\n",
        0o600,
    );
    let second = temp.file("second.env", "PRECEDENCE_API_KEY=from-second-file\n", 0o600);

    let mut warnings = Vec::new();
    let with_env = credentials::load(
        &temp.source(
            &[("PRECEDENCE_API_KEY", "from-environment")],
            &[first.clone(), second.clone()],
        ),
        &mut warnings,
    );
    assert!(
        with_env.value("PRECEDENCE_API_KEY").map(Secret::expose) == Some("from-environment"),
        "the environment wins over the files"
    );
    assert!(
        with_env.has("ONLY_HERE_API_KEY"),
        "key only in the first file"
    );
    assert!(warnings.is_empty(), "warnings: {warnings:?}");

    let without_env = credentials::load(&temp.source(&[], &[first, second]), &mut warnings);
    assert!(
        without_env.value("PRECEDENCE_API_KEY").map(Secret::expose) == Some("from-first-file"),
        "the first file in the list wins over the second"
    );
}

#[test]
fn an_empty_environment_value_does_not_block_the_file() {
    let temp = common::Temp::new("cred-empty");
    let file = temp.file("keys.env", "EMPTY_API_KEY=from-file\n", 0o600);
    let cred = credentials::load(
        &temp.source(&[("EMPTY_API_KEY", "")], &[file]),
        &mut Vec::new(),
    );
    assert!(cred.value("EMPTY_API_KEY").map(Secret::expose) == Some("from-file"));
}

#[test]
fn a_file_with_group_or_other_bits_is_not_read() {
    let denied = [0o644, 0o640, 0o604, 0o660, 0o606, 0o666];
    let accepted = [0o600, 0o400, 0o700];
    let temp = common::Temp::new("cred-modes");

    for mode in denied {
        let name = format!("denied-{mode:o}.env");
        let file = temp.file(&name, "DANGEROUS_API_KEY=value-that-must-not-leak\n", mode);
        let mut warnings = Vec::new();
        let cred = credentials::load(&temp.source(&[], &[file]), &mut warnings);
        assert!(
            !cred.has("DANGEROUS_API_KEY"),
            "mode {mode:o} should not be read"
        );
        assert_eq!(warnings.len(), 1, "mode {mode:o}");
        let warning = &warnings[0];
        assert!(
            warning.contains(&name),
            "the warning names the path: {warning}"
        );
        assert!(
            warning.contains("chmod 600"),
            "the warning says what to do: {warning}"
        );
        assert!(
            !warning.contains("value-that-must-not-leak"),
            "the warning must not contain the value"
        );
    }

    for mode in accepted {
        let name = format!("accepted-{mode:o}.env");
        let file = temp.file(&name, "GOOD_API_KEY=good-value\n", mode);
        let mut warnings = Vec::new();
        let cred = credentials::load(&temp.source(&[], &[file]), &mut warnings);
        assert!(cred.has("GOOD_API_KEY"), "mode {mode:o} should be read");
        assert!(warnings.is_empty(), "mode {mode:o}: warnings {warnings:?}");
    }
}

#[test]
fn a_missing_file_is_not_a_warning() {
    let temp = common::Temp::new("cred-missing");
    let mut warnings = Vec::new();
    let cred = credentials::load(
        &temp.source(&[], &[temp.root().join("does-not-exist.env")]),
        &mut warnings,
    );
    assert_eq!(cred.secret_count(), 0);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
}

#[test]
fn only_names_that_look_like_credentials_are_loaded() {
    assert!(looks_like_credential("TAVILY_API_KEY"));
    assert!(looks_like_credential("TWILIO_AUTH_TOKEN"));
    assert!(looks_like_credential("TWILIO_ACCOUNT_SID"));
    assert!(looks_like_credential("pushover_pass"));
    assert!(!looks_like_credential("USER"));
    assert!(!looks_like_credential("HOME"));

    let temp = common::Temp::new("cred-pattern");
    let cred = credentials::load(
        &temp.source(
            &[
                ("USER", "alice"),
                ("HOME", "/home/alice"),
                ("X_API_KEY", "x-fake"),
            ],
            &[],
        ),
        &mut Vec::new(),
    );
    assert!(!cred.has("USER"), "a user name is not a secret to mask");
    assert!(cred.value("X_API_KEY").map(Secret::expose) == Some("x-fake"));
    assert_eq!(cred.secret_count(), 1);
}

#[test]
fn in_an_env_file_only_names_that_look_like_credentials_are_loaded() {
    // The mirror of the previous test with the file as the source: a `.env`
    // shared with other tools brings variables that are not credentials, and
    // none of them may become a `Secret`.
    let temp = common::Temp::new("cred-pattern-file");
    let file = temp.file(
        "keys.env",
        "SOME_TOOL_DEBUG=0\nUSER=alice\nTAVILY_API_KEY=FAKE_test_key_0123456789\n",
        0o600,
    );
    let cred = credentials::load(&temp.source(&[], &[file]), &mut Vec::new());

    assert!(
        !cred.has("SOME_TOOL_DEBUG"),
        "`SOME_TOOL_DEBUG=0` is not a credential"
    );
    assert!(!cred.has("USER"), "a user name is not a secret to mask");
    assert!(
        cred.value("TAVILY_API_KEY").is_some(),
        "the Tavily key is loaded"
    );
    assert_eq!(cred.secret_count(), 1, "only the provider's key gets in");
}

#[test]
fn a_secret_is_never_printed() {
    let secret = Secret::new("TEST-SECRET-VALUE-c0f3");
    assert_eq!(format!("{secret:?}"), MARKER);
    assert_eq!(format!("{secret}"), MARKER);
    assert!(format!("{:?}", ("Authorization", &secret)).contains(MARKER));

    let temp = common::Temp::new("cred-debug");
    let file = temp.file("keys.env", "DEBUG_API_KEY=TEST-SECRET-VALUE-c0f3\n", 0o600);
    let cred = credentials::load(&temp.source(&[], &[file]), &mut Vec::new());
    let debug = format!("{cred:?}");
    assert!(
        !debug.contains("TEST-SECRET-VALUE-c0f3"),
        "the credentials' Debug shows no values"
    );
    assert!(debug.contains(MARKER));
    assert_eq!(cred.secret_count(), 1);
}

#[test]
fn opencode_go_comes_from_auth_json_when_missing_from_the_environment() {
    let temp = common::Temp::new("cred-opencode");
    temp.file(
        ".local/share/opencode/auth.json",
        r#"{"opencode-go":{"key":"fake-key-from-auth-json","type":"api"},"other":{"key":"do-not-use"}}"#,
        0o600,
    );
    let from_file = credentials::load(&temp.source(&[], &[]), &mut Vec::new());
    assert!(
        from_file.value(VAR_OPENCODE_GO).map(Secret::expose) == Some("fake-key-from-auth-json")
    );

    let from_env = credentials::load(
        &temp.source(&[(VAR_OPENCODE_GO, "fake-key-from-environment")], &[]),
        &mut Vec::new(),
    );
    assert!(
        from_env.value(VAR_OPENCODE_GO).map(Secret::expose) == Some("fake-key-from-environment")
    );
}

#[test]
fn claude_reads_token_and_expiry_and_ignores_the_rest_of_the_file() {
    let temp = common::Temp::new("cred-claude");
    let path = temp.file(".claude/.credentials.json", CLAUDE_CREDENTIALS, 0o600);
    let before = common::read(&path);

    let mut warnings = Vec::new();
    let cred = credentials::load(&temp.source(&[], &[]), &mut warnings);
    let claude = cred.claude().expect("Claude credential present");

    assert!(claude.token().expose() == "TEST-TOKEN-for-claude");
    assert_eq!(claude.expires_at_ms(), 1_790_000_000_000);
    assert!(
        claude.is_expired(ms(1_790_000_000_001)),
        "expired after the declared instant"
    );
    assert!(
        claude.is_expired(ms(1_790_000_000_000)),
        "at the declared instant it is already expired"
    );
    assert!(
        !claude.is_expired(ms(1_789_999_999_999)),
        "valid before the declared instant"
    );
    assert_eq!(cred.secret_count(), 1);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");

    let debug = format!("{cred:?}");
    assert!(!debug.contains("TEST-TOKEN-for-claude"));
    assert!(
        !debug.contains(REFRESH_DO_NOT_USE),
        "the refresh field is not stored"
    );
    assert!(
        !cred
            .all_exposed()
            .iter()
            .any(|s| s.expose() == REFRESH_DO_NOT_USE),
        "the refresh field does not enter the anti-leak guard"
    );
    assert!(
        !debug.contains("NOT-TO-BE-READ"),
        "only `claudeAiOauth` is read from the Claude file"
    );

    assert_eq!(
        common::read(&path),
        before,
        "the app never writes to credential files"
    );
}

#[test]
fn claude_without_usable_fields_warns_and_has_no_credential() {
    let temp = common::Temp::new("cred-claude-bad");
    temp.file(
        ".claude/.credentials.json",
        r#"{"claudeAiOauth":{"expiresAt":1}}"#,
        0o600,
    );
    let mut warnings = Vec::new();
    let cred = credentials::load(&temp.source(&[], &[]), &mut warnings);
    assert!(cred.claude().is_none());
    assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
    assert!(warnings[0].contains(".credentials.json"));
}

#[test]
fn a_subscription_file_with_an_open_mode_is_not_read() {
    let temp = common::Temp::new("cred-claude-mode");
    temp.file(".claude/.credentials.json", CLAUDE_CREDENTIALS, 0o644);
    let mut warnings = Vec::new();
    let cred = credentials::load(&temp.source(&[], &[]), &mut warnings);
    assert!(cred.claude().is_none());
    assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
    assert!(warnings[0].contains("chmod 600"));
}

#[test]
fn invalid_json_warns_without_panicking() {
    let temp = common::Temp::new("cred-bad-json");
    temp.file(".claude/.credentials.json", "{this is not json", 0o600);
    let mut warnings = Vec::new();
    let cred = credentials::load(&temp.source(&[], &[]), &mut warnings);
    assert!(cred.claude().is_none());
    assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
    assert!(warnings[0].contains("invalid JSON"));
}

#[test]
fn expands_the_tilde_in_config_paths() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let expected = home.expect("HOME is set").join(".config/quotop/keys.env");
    assert_eq!(expand_tilde("~/.config/quotop/keys.env"), expected);
    assert_eq!(
        expand_tilde("/absolute/path.env"),
        std::path::PathBuf::from("/absolute/path.env")
    );
    assert_eq!(dirs::home_dir().map(|h| expand_tilde("~") == h), Some(true));
}
