//! The single panic hook.
//!
//! It lives in a test binary of its own because the hook is **global to the
//! process**: `set_hook` is shared state, and in a binary with several tests
//! running in parallel there is no way to guarantee that the previous hook it
//! calls is the one this file installed.
//!
//! What is checked: inside a provider's zone the hook does **not** call the
//! previous hook (the panic payload never reaches stderr) and the engine still
//! converts the panic; outside the zone, a panic is handled as always.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use chrono::{DateTime, Utc};

use quotop::config::Config;
use quotop::credentials::Credentials;
use quotop::engine::{self, Context, Filter, PANIC_IN_PROVIDER};
use quotop::http::Http;
use quotop::model::{Category, Class, Cost, Level, Meter, Status, Unit};
use quotop::providers::{Provider, Response};

/// A provider that always blows up in `read`, with a recognizable payload —
/// where a real value could be.
struct Explodes;

impl Provider for Explodes {
    fn id(&self) -> &'static str {
        "explodes"
    }

    fn service(&self) -> &'static str {
        "Explodes"
    }

    fn category(&self) -> Category {
        Category::Search
    }

    fn class(&self) -> Class {
        Class::ExactBalance
    }

    fn cost(&self) -> Cost {
        Cost::Free
    }

    fn variables(&self) -> &'static [&'static str] {
        &[]
    }

    fn dashboard(&self) -> &'static str {
        "https://example.invalid"
    }

    fn read(&self, _http: &Http, _cred: &Credentials) -> Result<Vec<Response>, Status> {
        panic!("FAKE_provider_payload")
    }

    fn interpret(
        &self,
        _responses: &[Response],
        _now: DateTime<Utc>,
    ) -> Result<Vec<Meter>, Status> {
        Ok(vec![Meter {
            label: "never".to_string(),
            unit: Unit::Requests,
            currency: None,
            used: None,
            limit: None,
            remaining: None,
            resets_at: None,
            level: Level::Ok,
        }])
    }
}

#[test]
fn the_hook_is_silent_inside_a_provider_and_handles_panics_outside() {
    // The installed hook counts instead of printing: it is the "previous" hook
    // ours must call outside the zone — and must not call inside it.
    let panics = Arc::new(AtomicUsize::new(0));
    std::panic::set_hook({
        let panics = Arc::clone(&panics);
        Box::new(move |_| {
            panics.fetch_add(1, Ordering::SeqCst);
        })
    });
    quotop::panic_hook::install_panic_hook();

    // (a) A panic inside a provider: the engine turns it into
    // `unexpected_format` and the payload is never read.
    let http = Http::new(1);
    let cred = Credentials::default();
    let config = Config::default();
    let providers: Vec<Box<dyn Provider>> = vec![Box::new(Explodes)];
    let (tx, _rx) = mpsc::channel();
    let context = Context {
        http: &http,
        cred: &cred,
        config: &config,
        now: Utc::now(),
    };
    let refresh = engine::refresh(
        &providers,
        &Filter::Automatic,
        &BTreeMap::new(),
        &context,
        tx,
    );

    assert_eq!(refresh.readings.len(), 1);
    assert_eq!(
        refresh.readings[0].status,
        Status::UnexpectedFormat {
            message: PANIC_IN_PROVIDER.to_string()
        }
    );
    assert_eq!(
        panics.load(Ordering::SeqCst),
        0,
        "inside a provider's zone the previous hook is not called: the payload is not printed"
    );

    // (b) A panic outside the zone is a bug in the program and goes through the
    // previous hook.
    let outside = std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(|| panic!("FAKE_payload_outside_the_provider"));
    });
    outside
        .join()
        .expect("the panic is caught inside the thread");

    assert_eq!(
        panics.load(Ordering::SeqCst),
        1,
        "outside the zone the previous hook is still called"
    );
}
