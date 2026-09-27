//! Engine tests: parallelism, panic isolation, the cost filters, the 60 s
//! minimum interval, the merge with the previous reading and the
//! `config.toml` thresholds applied at the end.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use quotop::config::{Config, FileThresholds};
use quotop::credentials::Credentials;
use quotop::engine::{self, Context, Filter};
use quotop::http::Http;
use quotop::model::{Category, Class, Cost, Level, Meter, Reading, Status, Unit};
use quotop::providers::{Provider, Response};

// ------------------------------------------------------------------ fake

/// What a fake provider does when asked for a reading.
#[derive(Clone, Copy)]
enum Behaviour {
    Responds,
    PanicInRead,
    PanicInInterpret,
}

/// A fake provider: sleeps as long as the test asks, counts the requests and
/// does what it is told. With `cost = PerRequest` it stands in for
/// Brave/Context7.
struct Fake {
    id: &'static str,
    cost: Cost,
    sleep: Duration,
    behaviour: Behaviour,
    meter: Meter,
    requests: Arc<AtomicUsize>,
}

impl Fake {
    fn new(id: &'static str, cost: Cost, behaviour: Behaviour) -> Fake {
        Fake {
            id,
            cost,
            sleep: Duration::ZERO,
            behaviour,
            meter: Meter {
                label: "test".to_string(),
                unit: Unit::Requests,
                currency: None,
                used: Some(7.0),
                limit: Some(10.0),
                remaining: Some(3.0),
                resets_at: None,
                // 3/10 = 0.3: above the default warning (0.20), so `Ok`.
                level: Level::Ok,
            },
            requests: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn sleep(mut self, sleep: Duration) -> Fake {
        self.sleep = sleep;
        self
    }

    /// Shares the counter with the test, which is the one that reads it.
    fn counter(mut self, counter: Arc<AtomicUsize>) -> Fake {
        self.requests = counter;
        self
    }
}

impl Provider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }

    fn service(&self) -> &'static str {
        "Fake"
    }

    fn category(&self) -> Category {
        Category::Search
    }

    fn class(&self) -> Class {
        Class::ExactBalance
    }

    fn cost(&self) -> Cost {
        self.cost
    }

    fn variables(&self) -> &'static [&'static str] {
        &[]
    }

    fn dashboard(&self) -> &'static str {
        "https://example.invalid"
    }

    fn read(&self, _http: &Http, _cred: &Credentials) -> Result<Vec<Response>, Status> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if !self.sleep.is_zero() {
            std::thread::sleep(self.sleep);
        }
        match self.behaviour {
            Behaviour::PanicInRead => panic!("a fake provider blew up in `read`"),
            _ => Ok(vec![Response {
                http: 200,
                headers: Vec::new(),
                body: "{}".to_string(),
                duration_ms: 1,
            }]),
        }
    }

    fn interpret(
        &self,
        _responses: &[Response],
        _now: DateTime<Utc>,
    ) -> Result<Vec<Meter>, Status> {
        match self.behaviour {
            Behaviour::PanicInInterpret => panic!("a fake provider blew up in `interpret`"),
            _ => Ok(vec![self.meter.clone()]),
        }
    }
}

// ------------------------------------------------------------------ helpers

fn boxed(provider: Fake) -> Box<dyn Provider> {
    Box::new(provider)
}

fn context<'a>(
    http: &'a Http,
    cred: &'a Credentials,
    config: &'a Config,
    now: DateTime<Utc>,
) -> Context<'a> {
    Context {
        http,
        cred,
        config,
        now,
    }
}

/// A previous reading from the cache, with whatever status is needed.
fn previous_reading(provider: &str, read_at: DateTime<Utc>, status: Status) -> Reading {
    Reading {
        provider: provider.to_string(),
        service: "Fake".to_string(),
        category: Category::Search,
        class: Class::ExactBalance,
        cost: Cost::Free,
        read_at,
        duration_ms: Some(7),
        status,
        meters: Vec::new(),
    }
}

fn previous_map(reading: Reading) -> BTreeMap<String, Reading> {
    BTreeMap::from([(reading.provider.clone(), reading)])
}

fn reading_of<'a>(refresh: &'a engine::Refresh, id: &str) -> &'a Reading {
    refresh
        .readings
        .iter()
        .find(|l| l.provider == id)
        .expect("the provider's reading")
}

// ------------------------------------------------------------------ tests

#[test]
fn five_300_ms_providers_fit_in_under_1_s() {
    const IDS: [&str; 5] = ["fake_1", "fake_2", "fake_3", "fake_4", "fake_5"];

    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let now = Utc::now();
    let providers: Vec<Box<dyn Provider>> = IDS
        .iter()
        .map(|id| {
            boxed(Fake::new(id, Cost::Free, Behaviour::Responds).sleep(Duration::from_millis(300)))
        })
        .collect();

    let (tx, rx) = std::sync::mpsc::channel();
    let start = Instant::now();
    let refresh = engine::refresh(
        &providers,
        &Filter::Automatic,
        &BTreeMap::new(),
        &context(&http, &cred, &config, now),
        tx,
    );
    let elapsed = start.elapsed();

    assert_eq!(refresh.readings.len(), 5, "one reading per provider");
    assert_eq!(refresh.fetched.len(), 5);
    assert!(
        elapsed < Duration::from_millis(1000),
        "in series it would take 1500 ms; in parallel it took {elapsed:?}"
    );

    // Each reading goes out through the channel as soon as it is ready.
    let mut received = 0;
    while rx.try_recv().is_ok() {
        received += 1;
    }
    assert_eq!(received, 5);

    for reading in &refresh.readings {
        assert_eq!(reading.status, Status::Ok);
        assert_eq!(reading.meters.len(), 1);
        assert_eq!(reading.read_at, now);
        assert!(
            reading.duration_ms.is_some_and(|ms| ms >= 300),
            "the duration is the request's"
        );
    }
}

#[test]
fn a_panic_in_one_provider_does_not_stop_the_others() {
    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let now = Utc::now();
    let providers: Vec<Box<dyn Provider>> = vec![
        boxed(Fake::new(
            "panics_in_read",
            Cost::Free,
            Behaviour::PanicInRead,
        )),
        boxed(Fake::new(
            "panics_in_interpret",
            Cost::Free,
            Behaviour::PanicInInterpret,
        )),
        boxed(Fake::new("good", Cost::Free, Behaviour::Responds)),
    ];

    let (tx, _rx) = std::sync::mpsc::channel();
    let refresh = engine::refresh(
        &providers,
        &Filter::Automatic,
        &BTreeMap::new(),
        &context(&http, &cred, &config, now),
        tx,
    );

    assert_eq!(
        refresh.readings.len(),
        3,
        "all three arrive — the panicking ones too"
    );
    for id in ["panics_in_read", "panics_in_interpret"] {
        let reading = reading_of(&refresh, id);
        assert_eq!(
            reading.status,
            Status::UnexpectedFormat {
                message: engine::PANIC_IN_PROVIDER.to_string()
            },
            "{id}: the panic payload is never read, only the fixed message"
        );
        assert!(reading.meters.is_empty());
    }
    let good = reading_of(&refresh, "good");
    assert_eq!(good.status, Status::Ok);
    assert_eq!(good.meters.len(), 1);
}

#[test]
fn the_automatic_refresh_makes_no_requests_to_per_request_providers() {
    let free_counter = Arc::new(AtomicUsize::new(0));
    let paid_counter = Arc::new(AtomicUsize::new(0));
    let providers: Vec<Box<dyn Provider>> = vec![
        boxed(
            Fake::new("free", Cost::Free, Behaviour::Responds).counter(Arc::clone(&free_counter)),
        ),
        boxed(
            Fake::new("paid", Cost::PerRequest, Behaviour::Responds)
                .counter(Arc::clone(&paid_counter)),
        ),
    ];

    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let now = Utc::now();

    let (tx, _rx) = std::sync::mpsc::channel();
    let automatic = engine::refresh(
        &providers,
        &Filter::Automatic,
        &BTreeMap::new(),
        &context(&http, &cred, &config, now),
        tx,
    );
    assert_eq!(free_counter.load(Ordering::SeqCst), 1);
    assert_eq!(
        paid_counter.load(Ordering::SeqCst),
        0,
        "the `$` ones are not read automatically"
    );
    assert_eq!(automatic.filtered_out, ["paid"]);
    assert_eq!(automatic.fetched, ["free"]);
    assert_eq!(automatic.readings.len(), 1);
    assert_eq!(
        automatic.unread,
        ["paid"],
        "without a previous reading, the `$` one does not enter the cache — nothing is made up"
    );

    let (tx, _rx) = std::sync::mpsc::channel();
    let all = engine::refresh(
        &providers,
        &Filter::All,
        &BTreeMap::new(),
        &context(&http, &cred, &config, now),
        tx,
    );
    assert_eq!(
        paid_counter.load(Ordering::SeqCst),
        1,
        "`R`/`--include-paid` reads the `$` ones"
    );
    assert_eq!(all.readings.len(), 2);
    assert!(all.filtered_out.is_empty());
    assert!(all.unread.is_empty());
}

#[test]
fn an_unread_paid_provider_keeps_its_previous_cached_reading() {
    let counter = Arc::new(AtomicUsize::new(0));
    let providers: Vec<Box<dyn Provider>> = vec![
        boxed(Fake::new("free", Cost::Free, Behaviour::Responds)),
        boxed(
            Fake::new("paid", Cost::PerRequest, Behaviour::Responds).counter(Arc::clone(&counter)),
        ),
    ];
    let previous = previous_reading(
        "paid",
        Utc::now() - ChronoDuration::hours(3),
        Status::NoCredential,
    );

    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let (tx, _rx) = std::sync::mpsc::channel();
    let refresh = engine::refresh(
        &providers,
        &Filter::Automatic,
        &previous_map(previous.clone()),
        &context(&http, &cred, &config, Utc::now()),
        tx,
    );

    assert_eq!(counter.load(Ordering::SeqCst), 0);
    assert_eq!(
        refresh.readings.len(),
        2,
        "the `$` row stays, with its age visible"
    );
    assert_eq!(reading_of(&refresh, "paid"), &previous);
    assert_eq!(reading_of(&refresh, "paid").read_at, previous.read_at);
    assert_eq!(
        refresh.readings[0].provider, "free",
        "the order is the registry's"
    );
    assert!(refresh.unread.is_empty());
}

#[test]
fn a_provider_read_less_than_60_s_ago_is_not_read_again() {
    let counter = Arc::new(AtomicUsize::new(0));
    let providers: Vec<Box<dyn Provider>> = vec![boxed(
        Fake::new("fake_1", Cost::Free, Behaviour::Responds).counter(Arc::clone(&counter)),
    )];

    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let now = Utc::now();
    let ctx = context(&http, &cred, &config, now);
    let filter = Filter::One("fake_1".to_string());

    // 30 s: within the minimum interval — the request is not repeated.
    let recent = previous_reading("fake_1", now - ChronoDuration::seconds(30), Status::Ok);
    let (tx, _rx) = std::sync::mpsc::channel();
    let refresh = engine::refresh(&providers, &filter, &previous_map(recent.clone()), &ctx, tx);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    assert_eq!(refresh.within_interval, ["fake_1"]);
    assert!(refresh.fetched.is_empty());
    assert_eq!(refresh.readings, [recent]);

    // 61 s: it is read again.
    let old = previous_reading("fake_1", now - ChronoDuration::seconds(61), Status::Ok);
    let (tx, _rx) = std::sync::mpsc::channel();
    let refresh = engine::refresh(&providers, &filter, &previous_map(old), &ctx, tx);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert!(refresh.within_interval.is_empty());
    assert_eq!(refresh.fetched, ["fake_1"]);
    assert_eq!(refresh.readings[0].status, Status::Ok);

    // A `read_at` in the future (clock moved back) counts as recent.
    let future = previous_reading("fake_1", now + ChronoDuration::hours(1), Status::Ok);
    let (tx, _rx) = std::sync::mpsc::channel();
    let refresh = engine::refresh(&providers, &filter, &previous_map(future), &ctx, tx);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "when in doubt, no request is made"
    );
    assert_eq!(refresh.within_interval, ["fake_1"]);
}

#[test]
fn the_minimum_interval_and_the_filter_leave_one_reading_per_provider() {
    let counter = Arc::new(AtomicUsize::new(0));
    let providers: Vec<Box<dyn Provider>> = vec![
        boxed(Fake::new("recent", Cost::Free, Behaviour::Responds).counter(Arc::clone(&counter))),
        boxed(Fake::new("fresh", Cost::Free, Behaviour::Responds)),
        boxed(Fake::new(
            "never_read",
            Cost::PerRequest,
            Behaviour::Responds,
        )),
    ];
    let now = Utc::now();
    let previous = previous_map(previous_reading(
        "recent",
        now - ChronoDuration::seconds(10),
        Status::NoCredential,
    ));

    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let (tx, _rx) = std::sync::mpsc::channel();
    let refresh = engine::refresh(
        &providers,
        &Filter::Automatic,
        &previous,
        &context(&http, &cred, &config, now),
        tx,
    );

    assert_eq!(counter.load(Ordering::SeqCst), 0, "only `fresh` was read");
    assert_eq!(refresh.fetched, ["fresh"]);
    assert_eq!(refresh.within_interval, ["recent"]);
    assert_eq!(refresh.filtered_out, ["never_read"]);
    assert_eq!(refresh.unread, ["never_read"]);
    assert_eq!(
        refresh
            .readings
            .iter()
            .map(|l| l.provider.as_str())
            .collect::<Vec<_>>(),
        ["recent", "fresh"],
        "the order is the registry's and nothing is made up"
    );
    assert_eq!(reading_of(&refresh, "recent").status, Status::NoCredential);
    assert_eq!(reading_of(&refresh, "fresh").status, Status::Ok);
}

#[test]
fn the_engine_applies_the_config_thresholds_to_the_meters() {
    let providers: Vec<Box<dyn Provider>> =
        vec![boxed(Fake::new("fake_1", Cost::Free, Behaviour::Responds))];
    // 3 of 10 left: `Ok` by fraction (0.3) but `warning` in absolute terms (<= 5).
    let mut config = Config::default();
    config.thresholds.insert(
        "fake_1".to_string(),
        FileThresholds {
            warn_below: Some(5.0),
            critical_below: Some(1.0),
        },
    );

    let http = Http::new(1);
    let cred = Credentials::default();
    let now = Utc::now();

    let (tx, _rx) = std::sync::mpsc::channel();
    let by_default = engine::refresh(
        &providers,
        &Filter::Automatic,
        &BTreeMap::new(),
        &context(&http, &cred, &Config::default(), now),
        tx,
    );
    assert_eq!(by_default.readings[0].meters[0].level, Level::Ok);

    let (tx, _rx) = std::sync::mpsc::channel();
    let with_thresholds = engine::refresh(
        &providers,
        &Filter::Automatic,
        &BTreeMap::new(),
        &context(&http, &cred, &config, now),
        tx,
    );
    assert_eq!(with_thresholds.readings[0].meters[0].level, Level::Warning);
}

#[test]
fn the_registry_has_the_26_and_respects_the_disabled_ones() {
    let registry = engine::registry(&Config::default());
    let ids: Vec<&str> = registry.iter().map(|p| p.id()).collect();
    assert_eq!(ids.len(), 26);
    assert_eq!(ids[0], "tavily", "the registry order starts with Tavily");
    assert!(ids.contains(&"brave") && ids.contains(&"context7"));
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 26, "no repeated ids");

    let config = Config {
        disabled: vec!["brave".to_string(), "context7".to_string()],
        ..Config::default()
    };
    let registry = engine::registry(&config);
    assert_eq!(registry.len(), 24);
    assert!(
        !registry
            .iter()
            .any(|p| p.id() == "brave" || p.id() == "context7")
    );
}
