//! The refresh engine: one thread per provider, each `Reading` sent over
//! `mpsc` to the TUI as soon as it is ready, and the merge with what was
//! already in the cache.
//!
//! Four invariants live here:
//!
//! - **a panic in one provider does not bring down the others**: `read` +
//!   `interpret` run inside a `catch_unwind` and a panic becomes
//!   [`Status::UnexpectedFormat`] with the fixed message [`PANIC_IN_PROVIDER`].
//!   The panic payload is **never read nor printed** — it may contain a value,
//!   and the panic hook stays silent inside the provider zone, which is what
//!   [`in_provider`] reports;
//! - **messages come from outside** (an API's error body), so they go through
//!   [`guard::mask`] again before entering a `Reading`, even if the provider
//!   has already masked them;
//! - **a refresh never spends quota without an explicit request**:
//!   [`Filter::Automatic`] excludes the `cost = PerRequest` providers (Brave,
//!   Context7), and no provider is read twice within [`MIN_INTERVAL_S`]
//!   seconds;
//! - **nothing is made up**: a provider that was not read keeps its previous
//!   reading from the cache; if there is none either, it **does not enter**
//!   [`Refresh::readings`] (it shows up in [`Refresh::unread`], so the status
//!   bar can say so).
//!
//! The providers' `interpret` functions are pure and receive no
//! configuration, so each meter's level comes out of them with the default
//! thresholds; the `[thresholds.<provider>]` of `config.toml` are applied
//! **here**, in [`refresh`].

use std::cell::Cell;
use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::Sender;
use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::credentials::Credentials;
use crate::guard;
use crate::http::Http;
use crate::model::{self, Cost, Meter, Reading, Status};
use crate::providers::{self, Claude, Provider};
use crate::secret::Secret;

thread_local! {
    /// Whether the current thread is inside a provider's zone — the
    /// `catch_unwind` of [`read_one`]. The panic hook checks it to stay
    /// silent: there, a panic is a bad response from a service, not our bug.
    static IN_PROVIDER: Cell<bool> = const { Cell::new(false) };
}

/// Whether the current thread is reading a provider, inside the
/// `catch_unwind`.
///
/// The panic hook ([`crate::panic_hook`]) uses this to avoid printing the
/// payload of a provider's panic, which may contain a value.
pub fn in_provider() -> bool {
    IN_PROVIDER.with(Cell::get)
}

/// The message of a panic in a provider. It is **fixed**: the panic payload is
/// never read, formatted or printed, because it may contain a value; the panic
/// hook stays silent inside the provider zone.
pub const PANIC_IN_PROVIDER: &str = "panic in provider";

/// Minimum interval between two requests to the **same** provider, in
/// seconds. Protects the measured rate limits (Serper answers 50).
pub const MIN_INTERVAL_S: i64 = 60;

/// The id of the provider that [`registry`] swaps for a different
/// `User-Agent`.
const CLAUDE: &str = "claude";

/// What a refresh reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// Automatic refresh (at startup and every `interval_min`): only the free
    /// providers. The `cost = PerRequest` ones are never read without
    /// `r`/`R`.
    Automatic,
    /// Explicit full refresh (`R` in the TUI, `--include-paid` on the command
    /// line): includes the `$` ones.
    All,
    /// A single provider, by id (`r` on a row).
    One(String),
}

impl Filter {
    /// Whether this filter asks for a request to this provider.
    ///
    /// The filter decides **what is read**; the minimum interval is decided
    /// next, in [`within_interval`].
    fn includes(&self, provider: &dyn Provider) -> bool {
        match self {
            Filter::Automatic => provider.cost() == Cost::Free,
            Filter::All => true,
            Filter::One(id) => provider.id() == id,
        }
    }
}

/// Everything a refresh needs from outside, so that `main` and the tests can
/// feed the same engine from different sources.
pub struct Context<'a> {
    /// The HTTP client (a real one; in the tests the fake providers do not use
    /// it).
    pub http: &'a Http,
    /// This run's loaded credentials.
    pub cred: &'a Credentials,
    /// The effective configuration, for the per-provider thresholds.
    pub config: &'a Config,
    /// The instant of the refresh: it goes into `read_at` and decides the
    /// minimum interval (injectable so that the tests do not depend on the
    /// clock).
    pub now: DateTime<Utc>,
}

/// What a refresh did. The ids come out in registry order, so that the
/// summary and the warnings are deterministic.
#[derive(Debug, Clone, PartialEq)]
pub struct Refresh {
    /// One reading per registry provider: the fresh one, or the previous one
    /// from the cache. It may have fewer entries than the registry — the ids
    /// that have none are in [`Refresh::unread`].
    pub readings: Vec<Reading>,
    /// Ids that were read now.
    pub fetched: Vec<&'static str>,
    /// Ids the filter left out (no request at all).
    pub filtered_out: Vec<&'static str>,
    /// Ids not read because they are within the 60 s minimum interval.
    pub within_interval: Vec<&'static str>,
    /// Ids with neither a fresh nor a previous reading: not in `readings`.
    pub unread: Vec<&'static str>,
}

/// The effective registry: all 26 providers, minus the `disabled` ones of
/// `config.toml`, with Claude using the configured `claude_user_agent`.
///
/// A disabled provider is neither read nor shown: hiding works both ways — no
/// quota is spent reading what is not shown.
pub fn registry(config: &Config) -> Vec<Box<dyn Provider>> {
    providers::all()
        .into_iter()
        .filter(|provider| !config.disabled.iter().any(|id| id == provider.id()))
        .map(|provider| {
            if provider.id() == CLAUDE {
                Box::new(Claude::with_user_agent(&config.claude_user_agent)) as Box<dyn Provider>
            } else {
                provider
            }
        })
        .collect()
}

/// Refreshes the `providers` (in registry order) and returns the fresh
/// readings already merged with the `previous` ones.
///
/// Each fresh reading goes out through `tx` as soon as it is ready; `tx` is
/// consumed (one clone per thread) and is closed at the end, which the TUI can
/// use as "done".
pub fn refresh(
    providers: &[Box<dyn Provider>],
    filter: &Filter,
    previous: &BTreeMap<String, Reading>,
    context: &Context<'_>,
    tx: Sender<Reading>,
) -> Refresh {
    let mut fetched: Vec<&'static str> = Vec::new();
    let mut filtered_out: Vec<&'static str> = Vec::new();
    let mut within: Vec<&'static str> = Vec::new();

    let fresh: Vec<Reading> = std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for provider in providers {
            let id = provider.id();
            if !filter.includes(provider.as_ref()) {
                filtered_out.push(id);
                continue;
            }
            if within_interval(previous.get(id), context.now) {
                within.push(id);
                continue;
            }
            fetched.push(id);
            let tx = tx.clone();
            let provider = provider.as_ref();
            tasks.push((id, scope.spawn(move || read_one(provider, context, &tx))));
        }

        let mut fresh = Vec::with_capacity(tasks.len());
        for (id, task) in tasks {
            match task.join() {
                Ok(reading) => fresh.push(reading),
                // There is no expected path for the thread to blow up outside
                // the `catch_unwind`; if it happens, the provider gets the same
                // status as a panic and the others are not affected.
                Err(_) => {
                    if let Some(provider) = providers.iter().find(|p| p.id() == id) {
                        let status = Status::UnexpectedFormat {
                            message: PANIC_IN_PROVIDER.to_string(),
                        };
                        fresh.push(compose(provider.as_ref(), Err(status), context, 0));
                    }
                }
            }
        }
        fresh
    });

    let (readings, unread) = merge(providers, fresh, previous);
    Refresh {
        readings,
        fetched,
        filtered_out,
        within_interval: within,
        unread,
    }
}

/// Whether the previous reading is too recent to repeat the request.
///
/// A `read_at` in the future (clock moved back) counts as recent: when in
/// doubt, no request is made.
fn within_interval(previous: Option<&Reading>, now: DateTime<Utc>) -> bool {
    match previous {
        Some(reading) => now.signed_duration_since(reading.read_at).num_seconds() < MIN_INTERVAL_S,
        None => false,
    }
}

/// Reads **one** provider, inside its own thread.
fn read_one(provider: &dyn Provider, context: &Context<'_>, tx: &Sender<Reading>) -> Reading {
    let start = Instant::now();
    // The provider zone: while it lasts, a panic is not a bug in the program
    // and the panic hook prints nothing.
    IN_PROVIDER.with(|zone| zone.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let responses = provider.read(context.http, context.cred)?;
        provider.interpret(&responses, context.now)
    }));
    IN_PROVIDER.with(|zone| zone.set(false));
    let result = match result {
        Ok(result) => result,
        Err(_) => Err(Status::UnexpectedFormat {
            message: PANIC_IN_PROVIDER.to_string(),
        }),
    };
    let reading = compose(
        provider,
        result,
        context,
        start.elapsed().as_millis() as u64,
    );
    // No `Reading` is urgent for the TUI: if nobody is listening anymore, move
    // on.
    let _ = tx.send(reading.clone());
    reading
}

/// A provider's `Reading` from what `read`/`interpret` returned.
///
/// It is the only place where a reading is born: messages are masked here and
/// the `config.toml` thresholds are applied here.
fn compose(
    provider: &dyn Provider,
    result: Result<Vec<Meter>, Status>,
    context: &Context<'_>,
    duration_ms: u64,
) -> Reading {
    let secrets = context.cred.all_exposed();
    let (status, meters) = match result {
        Ok(meters) => (
            Status::Ok,
            with_thresholds(provider.id(), meters, context.config),
        ),
        Err(status) => (mask_status(status, &secrets), Vec::new()),
    };
    Reading {
        provider: provider.id().to_string(),
        service: provider.service().to_string(),
        category: provider.category(),
        class: provider.class(),
        cost: provider.cost(),
        read_at: context.now,
        duration_ms: Some(duration_ms),
        status,
        meters,
    }
}

/// Recomputes each meter's level with the configured thresholds.
///
/// Without an entry in `config.toml`, `thresholds_for` returns the defaults
/// and this changes nothing the provider computed.
fn with_thresholds(provider: &str, mut meters: Vec<Meter>, config: &Config) -> Vec<Meter> {
    let thresholds = config.thresholds_for(provider);
    for meter in &mut meters {
        meter.level = model::level(meter.used, meter.limit, meter.remaining, thresholds);
    }
    meters
}

/// The two statuses with free text come from outside; the others are our own
/// fixed categories and have nothing to mask.
fn mask_status(status: Status, secrets: &[&Secret]) -> Status {
    match status {
        Status::ApiError { http, message } => Status::ApiError {
            http,
            message: guard::mask(&message, secrets),
        },
        Status::UnexpectedFormat { message } => Status::UnexpectedFormat {
            message: guard::mask(&message, secrets),
        },
        other => other,
    }
}

/// Merges the fresh readings with the previous ones, in registry order.
///
/// Also returns the ids left with none (neither fresh nor previous): the
/// previous cache is disposable and may not have the provider, but the screen
/// row must not show a made-up value.
fn merge(
    providers: &[Box<dyn Provider>],
    fresh: Vec<Reading>,
    previous: &BTreeMap<String, Reading>,
) -> (Vec<Reading>, Vec<&'static str>) {
    let mut by_id: BTreeMap<String, Reading> = fresh
        .into_iter()
        .map(|reading| (reading.provider.clone(), reading))
        .collect();

    let mut readings = Vec::with_capacity(providers.len());
    let mut unread = Vec::new();
    for provider in providers {
        let id = provider.id();
        match by_id.remove(id) {
            Some(reading) => readings.push(reading),
            None => match previous.get(id) {
                Some(old) => readings.push(old.clone()),
                None => unread.push(id),
            },
        }
    }
    (readings, unread)
}
