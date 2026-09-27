//! Binary entry point: without `--json` it opens the TUI; with `--json` it
//! prints the readings to stdout and exits.
//!
//! Without an interactive terminal the TUI refuses to start, with a message and
//! exit code 2 (a screen that does not exist is not faked). An invalid
//! `config.toml` also refuses to start, with exit code 2.
//!
//! `--json` runs one refresh — of the free providers, or of all of them with
//! `--include-paid` — merges it into the previous cache, **saves the cache** and
//! prints that same text to stdout, exit code 0. The text comes from
//! [`cache::prepare`], which is what guarantees it went through the anti-leak
//! guard.
//!
//! Counts and states go to *stderr* (never values): how many providers the
//! filter left out, how many were spared by the minimum interval and how many
//! have no reading at all. That explains a reading count below 26 without
//! anyone having to guess.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::mpsc;

use chrono::Utc;

use quotop::cache;
use quotop::config;
use quotop::credentials::{self, Source};
use quotop::engine::{self, Context, Filter};
use quotop::http::Http;
use quotop::i18n;
use quotop::model::{CACHE_VERSION, Cache, Reading};
use quotop::t;

fn main() {
    std::process::exit(run());
}

/// The accepted options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Args {
    json: bool,
    include_paid: bool,
}

/// What the command line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Run(Args),
    Help,
    Version,
}

/// Parses the arguments. Anything that is not one of the known flags is an
/// error: a misspelled `--include-paid` must not quietly spend quota.
fn parse_args(line: &[String]) -> Result<Command, String> {
    let mut args = Args {
        json: false,
        include_paid: false,
    };
    for arg in line {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--json" => args.json = true,
            "--include-paid" => args.include_paid = true,
            other => return Err(t!("cli.unknown_argument", arg = other)),
        }
    }
    Ok(Command::Run(args))
}

/// Picks the interface language before anything else is printed, so that even
/// the configuration warnings come out in it: the choice saved in the
/// Preferences screen wins over `language` in `config.toml`.
fn set_language(warnings: &mut Vec<String>) {
    let saved = quotop::tui::theme::state_path(quotop::tui::theme::FILE_NAME)
        .and_then(|path| i18n::language_in_file(&path));
    let configured = config::default_path().and_then(|path| i18n::language_in_file(&path));
    i18n::set_current(i18n::resolve(
        configured.as_deref(),
        saved.as_deref(),
        warnings,
    ));
}

fn run() -> i32 {
    let mut warnings = Vec::new();
    set_language(&mut warnings);

    let line: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&line) {
        Ok(Command::Run(args)) => args,
        Ok(Command::Help) => {
            println!("{}", t!("cli.help", version = env!("CARGO_PKG_VERSION")));
            return 0;
        }
        Ok(Command::Version) => {
            println!("quotop {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        Err(error) => {
            eprintln!("quotop: {error}");
            eprintln!("{}", t!("cli.usage"));
            return 2;
        }
    };

    let loaded = match config::load() {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("quotop: {error}");
            return 2;
        }
    };

    // A single panic hook for both modes. It is installed after the command
    // line has been validated — a usage error is not a panic — and before the
    // TUI opens, since the TUI is what switches the terminal to raw mode.
    quotop::panic_hook::install_panic_hook();
    warnings.extend(loaded.warnings.iter().cloned());
    let source = Source::from_env(&loaded.config);
    let credentials = credentials::load(&source, &mut warnings);

    if !args.json {
        // The TUI is the normal mode; it refuses to start by itself when there
        // is no terminal.
        return quotop::tui::run(quotop::tui::Startup {
            config: loaded.config,
            credentials,
            source,
            warnings,
            cache_path: cache::default_path(),
            include_paid: args.include_paid,
        });
    }

    let now = Utc::now();
    let secrets = credentials.all_exposed();
    let filter = if args.include_paid {
        Filter::All
    } else {
        Filter::Automatic
    };
    let providers = engine::registry(&loaded.config);

    // The cache is the starting point: whatever is not read now keeps what is
    // already there.
    let cache_path = cache::default_path();
    let mut previous: BTreeMap<String, Reading> = BTreeMap::new();
    if let Some(path) = &cache_path
        && let Some(cached) = cache::read(path, &secrets, &mut warnings)
    {
        for reading in cached.readings {
            previous.insert(reading.provider.clone(), reading);
        }
    }

    let http = Http::new(loaded.config.timeout_s);
    let context = Context {
        http: &http,
        cred: &credentials,
        config: &loaded.config,
        now,
    };
    // The receiver stays alive (a reading that arrives after the refresh is
    // dropped by the channel, not an error).
    let (tx, _rx) = mpsc::channel();
    let refresh = engine::refresh(&providers, &filter, &previous, &context, tx);

    for warning in &warnings {
        eprintln!("quotop: {}", t!("cli.warning", message = warning));
    }
    if filter == Filter::Automatic && !refresh.filtered_out.is_empty() {
        eprintln!(
            "quotop: {}",
            i18n::plural(
                "cli.filtered_out",
                refresh.filtered_out.len(),
                &[("ids", refresh.filtered_out.join(", "))]
            )
        );
    }
    if !refresh.within_interval.is_empty() {
        eprintln!(
            "quotop: {}",
            i18n::plural(
                "cli.within_interval",
                refresh.within_interval.len(),
                &[
                    ("seconds", engine::MIN_INTERVAL_S.to_string()),
                    ("ids", refresh.within_interval.join(", ")),
                ]
            )
        );
    }
    if !refresh.unread.is_empty() {
        eprintln!(
            "quotop: {}",
            i18n::plural(
                "cli.unread",
                refresh.unread.len(),
                &[("ids", refresh.unread.join(", "))]
            )
        );
    }

    let new = Cache {
        version: CACHE_VERSION,
        generated_at: now,
        readings: refresh.readings,
    };
    let text = match cache::prepare(&new, &secrets) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("quotop: {}", t!("cli.nothing_written", error = error));
            return 2;
        }
    };

    // A closed pipe (`quotop --json | head -1`) is not a failure of the
    // refresh: the cache is still saved and the exit is clean. Any other write
    // error is a lost result — `>` into `/dev/full`, a full disk or a dead
    // descriptor — and the exit code must say so instead of reporting success.
    let mut code = 0;
    if let Err(error) = write_stdout(&text) {
        let message = t!("cli.stdout_failed", error = error);
        eprintln!("quotop: {}", t!("cli.warning", message = message));
        if error.kind() != std::io::ErrorKind::BrokenPipe {
            code = 1;
        }
    }

    // The cache is disposable: if it cannot be saved, warn and move on — the
    // result is already on stdout.
    if let Some(path) = &cache_path
        && let Err(error) = cache::save(path, &text)
    {
        eprintln!("quotop: {}", t!("cli.warning", message = error));
    }

    code
}

/// Writes the `Cache` to stdout without panicking: `println!` panics if the
/// consumer closes the pipe.
fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut output = std::io::stdout().lock();
    writeln!(output, "{text}")?;
    output.flush()
}
