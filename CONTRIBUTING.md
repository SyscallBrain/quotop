# Contributing to quotop

Thanks for your interest! Bug reports, new services, translations and fixes are
all welcome. This guide covers how the project is organised, how to run it, and
step-by-step recipes for the two most common contributions: adding a service
and adding a language.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues should **not** go in public issues — see
[SECURITY.md](SECURITY.md).

## Getting started

You need Rust 1.88 or newer (`rustup` is the easiest way to get it).

```sh
git clone https://github.com/SyscallBrain/quotop
cd quotop
cargo run                  # the TUI
cargo run -- --json        # JSON output
cargo test                 # the whole test suite (no network needed)
```

Before opening a pull request, please make sure these pass — CI runs the same:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests never touch the network or your real keys: providers are tested
against recorded responses in `tests/fixtures/`, and the TUI is drawn into an
in-memory terminal.

## Project layout

```
src/
├── main.rs            command line: TUI or --json
├── model.rs           the data model: Reading, Meter, Status, Level, …
├── engine.rs          refresh: reads providers in parallel, cache fallback
├── providers/         one file per service (+ undocumented/ for private APIs)
├── credentials.rs     where keys come from (env, key files, tool files)
├── secret.rs          the Secret type keys live in
├── guard.rs           the leak guard (masks keys in API errors, blocks output)
├── keyfile.rs         writing a key typed in the Services screen
├── http.rs            HTTP client wrapper (timeouts, no auto-errors)
├── cache.rs           ~/.cache/quotop/last.json
├── config.rs          ~/.config/quotop/config.toml
├── i18n.rs            translations (see locales/)
└── tui/               the terminal interface (ratatui)
locales/               one TOML file per language
tests/                 integration tests and fixtures
```

A few principles the code follows — please keep them:

- **Read-only.** Providers only call usage/balance endpoints.
- **Never spend quota silently.** A provider whose read costs a request of the
  user's quota declares `Cost::PerRequest`, and automatic refreshes skip it.
- **Keys stay inside `Secret`.** Never format, log or store a key; only
  `http.rs` exposes a secret, to build the request.
- **Don't invent numbers.** If a service doesn't report something, the meter
  leaves it as `None` and the screen shows "—" rather than a guess.
- **User-facing text goes through i18n** (`t!("…")`), JSON output and internal
  diagnostics stay in English.

## Adding a service

Each service is one type implementing the `Provider` trait in
`src/providers/`. `src/providers/tavily.rs` is a good minimal example.

1. **Create `src/providers/<id>.rs`** with a unit struct and implement
   `Provider`:

   | Method | What to return |
   | --- | --- |
   | `id` | A short, stable, lowercase id (`"tavily"`); it is used in `config.toml` and `--json`. |
   | `service` | The display name (`"Tavily"`). |
   | `category` | `Search`, `Llm`, `Subscription` or `Other`. |
   | `class` | `ExactBalance`, `UsageVsLimit`, `RateLimitOnly` or `NoApi`. |
   | `cost` | `Cost::Free` (default) or `Cost::PerRequest` if reading uses quota. |
   | `variables` | The environment variables holding the key(s), e.g. `&["TAVILY_API_KEY"]`. |
   | `dashboard` | The URL where the user can see the balance. |
   | `endpoint` | The URL quotop calls, **without** query string (shown in the detail panel). |
   | `read` | Make the HTTP request(s) and return the raw responses. |
   | `interpret` | Turn the responses into `Meter`s (no network here). |

   Split the work the same way the existing providers do: `read` only fetches
   (use the helpers in `providers/mod.rs`: `credential`, `request`,
   `Request::get`, `bearer`, …), and `interpret` only parses, so it can be
   tested without a network. Build meters with `MeterBuilder`, give them a short
   **English** label (`"credits"`, `"monthly plan"`, `"5h"`) and return a
   `Status` error (`bad_format(…)`) when the response doesn't look as expected.

2. **Register it**: add `mod <id>;` and an entry in `all()` in
   `src/providers/mod.rs` (the order there is the order on screen).

3. **Translate the labels** your meters use: add them to the `[labels]` table
   of every non-English locale (`locales/pt-PT.toml`), e.g.
   `"monthly plan" = "plano mensal"`.

4. **Test it.** Record a real response (remove anything personal: account ids,
   emails, names), save it under `tests/fixtures/`, and add a test that feeds it
   to `interpret` and checks the meters — see `tests/synthetic.rs` for examples.
   If you only have the documented response format, say so in the fixture.

5. **Document it**: add a row to the services table in `README.md`.

If the service has no official usage API and you rely on an endpoint its own
apps use, put it in `src/providers/undocumented/` and add its id to
`UNDOCUMENTED` so the screen marks it with `†`.

## Adding a language

Translations live in `locales/<code>.toml`, one file per language, using BCP 47
codes (`en-US`, `pt-PT`, `es-ES`, `pt-BR`, …).

1. Copy `locales/en-US.toml` to `locales/<code>.toml` and translate every value.
   - Keep the keys exactly as they are.
   - Keep `{placeholders}` unchanged; you can move them around in the sentence.
   - Counted texts have `.one` and `.other` forms; `{n}` is the number.
   - Set `meta.decimal_separator` (`"."` or `","`).
   - Translate the meter labels in the `[labels]` table (copy it from
     `locales/pt-PT.toml` to see which labels exist).
   - Mind the width: the level column fits 8 characters (`CRITICAL`), and the
     screen must still work at 60 columns.
2. Add one entry to `LANGUAGES` in `src/i18n.rs`:

   ```rust
   Language {
       code: "es-ES",
       name: "Español (España)",
       source: include_str!("../locales/es-ES.toml"),
   },
   ```

3. Run `cargo test`. `tests/i18n.rs` fails if a key or placeholder is missing or
   extra, and tells you which. Then run quotop, press `p`, `l` to switch to
   your language and look through every screen (`?`, `Enter`, `s`, `p`).

If you change or add a user-facing text anywhere in the code, add its key to
**every** locale file (English is the reference); the tests will remind you.

## Pull requests

- Keep each pull request focused on one change, and describe what and why.
- Add or update tests for behaviour changes.
- Update `README.md` and `CHANGELOG.md` (under "Unreleased") when users would
  notice the change.
- Write code, comments and commit messages in English.
