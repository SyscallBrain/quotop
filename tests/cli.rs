//! The command-line contract. In the tests stdout is a *pipe*, so the binary
//! is always "without an interactive terminal": that is the path exercised
//! here. A real `quotop --json` would make network requests, so it is only
//! run here with no credentials at all.
//!
//! The child's environment is confined to a temporary directory (HOME,
//! XDG_*): neither the user's real `config.toml` nor their cache is touched.

mod common;

use std::path::Path;
use std::process::{Command, Output};

fn quotop(temp: &common::Temp, args: &[&str]) -> Output {
    let root = temp.root();
    Command::new(env!("CARGO_BIN_EXE_quotop"))
        .args(args)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .expect("run the binary")
}

/// The same, with stdout connected to `target` instead of a *pipe*.
///
/// The child's environment is cleared (`env_clear`, no keys at all): no
/// provider has a credential to use, so the refresh neither goes to the
/// network nor spends quota.
fn quotop_with_stdout_to(temp: &common::Temp, args: &[&str], target: &Path) -> Output {
    let root = temp.root();
    let file = std::fs::File::create(target).expect("open the stdout target");
    Command::new(env!("CARGO_BIN_EXE_quotop"))
        .args(args)
        .env_clear()
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("PATH", "/usr/bin:/bin")
        .stdout(std::process::Stdio::from(file))
        .output()
        .expect("run the binary")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn without_json_and_without_a_terminal_it_refuses_with_code_2() {
    let temp = common::Temp::new("cli-no-json");
    let output = quotop(&temp, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).contains("--json"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_unknown_argument_refuses_with_code_2_without_spending_quota() {
    let temp = common::Temp::new("cli-bad-argument");
    let output = quotop(&temp, &["--everything"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).contains("quotop: unknown argument `--everything`"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("usage: quotop"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn help_and_version_print_to_stdout_with_code_0() {
    let temp = common::Temp::new("cli-help");
    for flag in ["-h", "--help"] {
        let output = quotop(&temp, &[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert!(
            stdout(&output).contains("Usage: quotop"),
            "{flag}: {}",
            stdout(&output)
        );
        assert!(
            stdout(&output).contains("--include-paid"),
            "{flag}: {}",
            stdout(&output)
        );
        assert!(output.stderr.is_empty(), "{flag}: {}", stderr(&output));
    }
    for flag in ["-V", "--version"] {
        let output = quotop(&temp, &[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert_eq!(
            stdout(&output),
            format!("quotop {}\n", env!("CARGO_PKG_VERSION")),
            "{flag}"
        );
    }
}

#[test]
fn the_language_is_set_before_anything_is_printed() {
    let temp = common::Temp::new("cli-language");
    temp.file("config/quotop/config.toml", "language = \"pt-PT\"\n", 0o644);

    let output = quotop(&temp, &["--everything"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("argumento desconhecido `--everything`"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_invalid_config_toml_refuses_to_start_with_code_2() {
    let temp = common::Temp::new("cli-bad-config");
    // Valid TOML, wrong type: that is what makes the app refuse to start. An
    // unknown key would only be a warning — and would let `--json` go to the
    // network.
    temp.file(
        "config/quotop/config.toml",
        "interval_min = \"fifteen\"\n",
        0o644,
    );

    let output = quotop(&temp, &["--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "refuses without writing anything to stdout"
    );
    assert!(
        stderr(&output).contains("config.toml"),
        "the error names the file: {}",
        stderr(&output)
    );
}

#[test]
fn json_that_cannot_be_written_exits_with_code_1() {
    // `>` into `/dev/full` opens fine and the write fails with `ENOSPC` — it is
    // not a closed pipe, it is a lost result, and the exit code must say so.
    if !Path::new("/dev/full").exists() {
        eprintln!("no /dev/full on this system; skipped");
        return;
    }

    let temp = common::Temp::new("cli-dev-full");
    let output = quotop_with_stdout_to(&temp, &["--json"], Path::new("/dev/full"));

    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("could not write to stdout"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_binary_lives_in_src_main_rs() {
    // Harness sanity check: `CARGO_BIN_EXE_*` only exists for integration
    // tests, and it is what guarantees we are running the project's binary.
    assert!(Path::new(env!("CARGO_BIN_EXE_quotop")).exists());
}
