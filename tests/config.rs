//! `config.toml` tests: defaults, unknown keys (a warning) and invalid TOML
//! (an error with the line; startup refused).

mod common;

use quotop::config::{self, Config, ConfigError};
use quotop::model::Thresholds;

const FULL_CONFIG: &str = r#"
interval_min = 0
timeout_s = 20
key_files = ["/tmp/one.env", "/tmp/two.env"]
disabled = ["brave", "context7"]
claude_user_agent = "claude-code/9.9.9"

[thresholds]
[thresholds.openrouter]
warn_below = 5.0
critical_below = 1.0
"#;

#[test]
fn defaults_are_the_documented_ones() {
    let c = Config::default();
    assert_eq!(c.interval_min, 15);
    assert_eq!(c.timeout_s, 10);
    assert_eq!(c.key_files, ["~/.config/quotop/keys.env"]);
    assert!(c.disabled.is_empty());
    assert_eq!(c.claude_user_agent, "claude-code/2.1.282");
    assert!(c.thresholds.is_empty());
    assert_eq!(c.thresholds_for("openrouter"), Thresholds::default());
}

#[test]
fn valid_config_reads_every_field() {
    let temp = common::Temp::new("config-ok");
    let path = temp.file("config.toml", FULL_CONFIG, 0o600);
    let loaded = config::load_from(&path).expect("valid config");

    assert!(
        loaded.warnings.is_empty(),
        "warnings: {:?}",
        loaded.warnings
    );
    assert_eq!(loaded.origin.as_deref(), Some(path.as_path()));
    let c = loaded.config;
    assert_eq!(c.interval_min, 0, "0 turns the automatic refresh off");
    assert_eq!(c.timeout_s, 20);
    assert_eq!(c.key_files.len(), 2);
    assert_eq!(c.disabled, ["brave", "context7"]);
    assert_eq!(c.claude_user_agent, "claude-code/9.9.9");
    assert_eq!(
        c.thresholds_for("openrouter"),
        Thresholds::Absolute {
            warning: 5.0,
            critical: 1.0
        }
    );
    assert_eq!(c.thresholds_for("tavily"), Thresholds::default());
}

#[test]
fn partial_thresholds_do_not_make_up_the_missing_one() {
    let temp = common::Temp::new("config-thresholds");
    let path = temp.file(
        "config.toml",
        "[thresholds.serper]\nwarn_below = 100.0\n\n[thresholds.jina]\ncritical_below = 10.0\n",
        0o600,
    );
    let c = config::load_from(&path).expect("valid config").config;
    assert_eq!(
        c.thresholds_for("serper"),
        Thresholds::Absolute {
            warning: 100.0,
            critical: 0.0
        },
        "without `critical_below` there is no absolute critical level"
    );
    assert_eq!(
        c.thresholds_for("jina"),
        Thresholds::Absolute {
            warning: 10.0,
            critical: 10.0
        },
        "without `warn_below` there is no warning zone"
    );
}

#[test]
fn an_unknown_key_warns_and_does_not_prevent_startup() {
    let temp = common::Temp::new("config-unknown");
    let path = temp.file(
        "config.toml",
        "interval_min = 5\ninactivity = 3\n\n[thresholds.openrouter]\nwarn_below = 5.0\nabyss = 1.0\n",
        0o600,
    );
    let loaded = config::load_from(&path).expect("valid config with extra keys");
    assert_eq!(loaded.config.interval_min, 5, "the good keys still apply");
    assert_eq!(
        loaded.config.thresholds_for("openrouter"),
        Thresholds::Absolute {
            warning: 5.0,
            critical: 0.0
        }
    );
    let warnings = loaded.warnings.join("\n");
    assert!(
        warnings.contains("unknown key `inactivity`"),
        "warnings: {warnings}"
    );
    assert!(
        warnings.contains("`thresholds.openrouter.abyss`"),
        "warnings: {warnings}"
    );
    assert_eq!(loaded.warnings.len(), 2);
}

#[test]
fn invalid_toml_is_an_error_with_the_line() {
    let temp = common::Temp::new("config-invalid");
    let path = temp.file(
        "config.toml",
        "interval_min = 15\ntimeout_s = 10\nunclosed = [1, 2",
        0o600,
    );
    let error = config::load_from(&path).expect_err("should refuse");
    assert!(matches!(error, ConfigError::Toml { .. }), "{error:?}");
    assert_eq!(error.line(), Some(3), "error: {error}");
    assert_eq!(error.path(), path.as_path());
    assert!(error.to_string().contains("line 3"), "{error}");
}

#[test]
fn a_key_with_the_wrong_type_also_gives_the_line() {
    let temp = common::Temp::new("config-wrong-type");
    let path = temp.file(
        "config.toml",
        "timeout_s = 10\ninterval_min = \"fifteen\"\n",
        0o600,
    );
    let error = config::load_from(&path).expect_err("should refuse");
    assert!(matches!(error, ConfigError::Toml { .. }), "{error:?}");
    assert_eq!(error.line(), Some(2), "error: {error}");
}

/// A missing default file is not an error (that path is `config::load`, which
/// reads the real user's configuration, so it is exercised through the binary
/// in `tests/cli.rs`, with `HOME`/`XDG_CONFIG_HOME` in a temporary directory).
/// A file asked for explicitly, though, is a read error — not a silent default.
#[test]
fn an_explicitly_requested_missing_file_is_a_read_error() {
    let temp = common::Temp::new("config-no-file");
    let path = temp.root().join("config.toml");
    let error = config::load_from(&path).expect_err("should refuse");
    assert!(matches!(error, ConfigError::Read { .. }), "{error:?}");
    assert!(error.to_string().starts_with("could not read"), "{error}");
}

/// The repository's `config.example.toml` is what the README says to copy: if
/// it stops being valid TOML, the example lies — this test prevents that.
#[test]
fn the_repository_example_config_is_valid() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
    let loaded = config::load_from(&path).expect("the example must be valid");
    assert!(
        loaded.warnings.is_empty(),
        "the example cannot have unknown keys: {:?}",
        loaded.warnings
    );
    // The example's values are the defaults, and the per-service thresholds are
    // there to show their shape (in the meter's units, not a fraction).
    let c = loaded.config;
    assert_eq!(c.interval_min, 15);
    assert_eq!(c.timeout_s, 10);
    assert_eq!(c.claude_user_agent, "claude-code/2.1.282");
    assert_eq!(
        c.thresholds_for("openrouter"),
        Thresholds::Absolute {
            warning: 5.0,
            critical: 1.0,
        }
    );
    // Only the warning is written: the critical one stays at 0.0 (it only
    // fires at zero).
    assert_eq!(
        c.thresholds_for("jina"),
        Thresholds::Absolute {
            warning: 1_000_000.0,
            critical: 0.0,
        }
    );
    // Whoever is not in the example gets the default thresholds.
    assert_eq!(c.thresholds_for("tavily"), Thresholds::default());
}
