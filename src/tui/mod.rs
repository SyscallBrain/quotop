//! The TUI: raw mode, alternate screen, one frame per event.
//!
//! The terminal is restored on four paths — `q`, `Ctrl+C`, a
//! `SIGTERM`/`SIGHUP` (which the loop treats as a normal exit) and a panic
//! (through the single hook in [`crate::panic_hook`]) — because leaving it in
//! raw mode breaks the shell afterwards. The cache is read before the screen
//! starts (so the first frame already shows what was there) and written at the
//! end of every refresh.

pub mod app;
pub mod detail;
pub mod preferences_panel;
pub mod services_panel;
pub mod style;
pub mod theme;
pub mod view;
pub mod visibility;

pub use app::{App, MIN_HEIGHT, MIN_WIDTH, RefreshEvent, Row, Sources, View};

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use chrono::Utc;
use crossterm::cursor::Hide;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use signal_hook::consts::{SIGHUP, SIGTERM};

use crate::cache;
use crate::config::Config;
use crate::credentials::{Credentials, Source};
use crate::engine::{self, Filter};
use crate::panic_hook;
use crate::t;

/// What the TUI receives from the code that already read the configuration
/// and the credentials.
pub struct Startup {
    pub config: Config,
    pub credentials: Credentials,
    /// Where the credentials were read from: the Services & keys screen reads
    /// them again from here after saving a key.
    pub source: Source,
    /// The warnings gathered so far (configuration and credentials), for the
    /// screen to show.
    pub warnings: Vec<String>,
    /// Where the cache lives; `None` when there is nowhere to save it (not an
    /// error).
    pub cache_path: Option<PathBuf>,
    /// Include paid services: the first refresh reads everything; without it,
    /// it is the automatic one, which reads only the free services.
    pub include_paid: bool,
}

/// Runs the TUI until the user quits, and returns the exit code.
pub fn run(startup: Startup) -> i32 {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        eprintln!("{}", t!("tui.run.no_terminal"));
        return 2;
    }
    let now = Utc::now();
    let sources = Sources {
        providers: Arc::new(engine::registry(&startup.config)),
        config: Arc::new(startup.config),
        cred: Arc::new(startup.credentials),
    };
    let mut warnings = startup.warnings;
    let previous = match startup.cache_path.as_ref() {
        Some(path) => {
            let secrets = sources.cred.all_exposed();
            cache::read(path, &secrets, &mut warnings)
        }
        None => None,
    };
    let initial_filter = if startup.include_paid {
        Filter::All
    } else {
        Filter::Automatic
    };
    let mut app = App::new(sources, previous, now, warnings, startup.cache_path);
    app.load_preferences(theme::default_path());
    app.attach_config(
        theme::state_path(visibility::FILE_NAME),
        Some(startup.source),
    );

    // Signals are registered before the terminal enters raw mode, so any
    // warning is printed on a normal terminal.
    let signals = Signals::register();
    let guard = match TerminalGuard::install() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("{}", t!("tui.run.terminal_failed", error = error));
            return 2;
        }
    };
    let mut terminal = match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
        Ok(terminal) => terminal,
        Err(error) => {
            eprintln!("{}", t!("tui.run.screen_failed", error = error));
            drop(guard);
            return 2;
        }
    };

    let (tx, rx) = mpsc::channel();
    app.launch(initial_filter, tx.clone());
    let code = event_loop(&mut terminal, &mut app, &rx, &tx, &signals);
    drop(terminal);
    drop(guard);
    code
}

/// The loop: draw, take in what arrived from the refresh, read the keyboard
/// (with a short timeout, so readings show up as they arrive) and launch the
/// automatic refresh when it is due.
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    rx: &mpsc::Receiver<RefreshEvent>,
    tx: &mpsc::Sender<RefreshEvent>,
    signals: &Signals,
) -> i32 {
    loop {
        app.now = Utc::now();
        if let Err(error) = terminal.draw(|frame| view::draw(frame, app)) {
            eprintln!("{}", t!("tui.run.draw_failed", error = error));
            return 2;
        }
        for refresh_event in rx.try_iter() {
            match refresh_event {
                RefreshEvent::Reading(reading) => app.receive(*reading),
                RefreshEvent::Done { refresh, now } => app.finish(*refresh, now),
            }
        }
        // A `SIGTERM`/`SIGHUP` exits here, through the normal path: the
        // terminal is restored by the `TerminalGuard`'s `Drop`.
        if let Some(code) = signals.exit_code() {
            return code;
        }
        if app.quit {
            return 0;
        }
        match event::poll(Duration::from_millis(200)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) => {
                    if let Some(filter) = handle_key(app, key) {
                        app.launch(filter, tx.clone());
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("{}", t!("tui.run.keyboard_failed", error = error));
                    return 2;
                }
            },
            Ok(false) => {}
            Err(error) => {
                eprintln!("{}", t!("tui.run.keyboard_failed", error = error));
                return 2;
            }
        }
        if let Some(filter) = app.refresh_due(app.now) {
            app.launch(filter, tx.clone());
        }
        if !app.refreshing
            && let Some(filter) = app.pending.take()
        {
            app.launch(filter, tx.clone());
        }
    }
}

/// Maps a key to its action in the current view. Returns a read request when
/// the key asks for a refresh (`r` and `R`), which the caller launches.
///
/// Public because it is the view's key map: the tests drive it with
/// `KeyEvent`s, without a terminal.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Option<Filter> {
    // Some terminals also send the key release: only the press counts.
    if matches!(key.kind, KeyEventKind::Release) {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        app.quit = true;
        return None;
    }
    // While a refresh is running, `r`/`R` would only ask again for what is
    // already being read: the header shows `reading…` and the keys wait.
    let request = |app: &App, filter: Filter| (!app.refreshing).then_some(filter);
    // What the Services screen asks for (a new key, a service that became
    // visible) is not lost because a refresh is running: it is kept pending.
    let from_menu = |app: &mut App, filter: Option<Filter>| {
        let filter = filter?;
        if app.refreshing {
            app.pending = Some(filter);
            return None;
        }
        Some(filter)
    };
    // While typing a key, every key is text (a `q` inside a key must not quit
    // the app).
    if app.view == View::Services && app.menu.entry.is_some() {
        match key.code {
            KeyCode::Enter => {
                let filter = app.confirm_entry();
                return from_menu(app, filter);
            }
            KeyCode::Esc => app.cancel_entry(),
            KeyCode::Backspace => app.backspace(),
            KeyCode::Char(character) => app.type_char(character),
            _ => {}
        }
        return None;
    }
    match app.view {
        View::List => match key.code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Char('j') | KeyCode::Down => app.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => app.move_by(-1),
            KeyCode::Char('g') | KeyCode::Home => app.move_by(isize::MIN),
            KeyCode::Char('G') | KeyCode::End => app.move_by(isize::MAX),
            KeyCode::Enter | KeyCode::Char('l') => app.view = View::Detail,
            KeyCode::Char('r') => {
                let filter = app.selection_request()?;
                return request(app, filter);
            }
            KeyCode::Char('R') => return request(app, Filter::All),
            KeyCode::Char('f') => app.toggle_filter(),
            KeyCode::Char('p') => app.open_preferences(),
            KeyCode::Char('s') => app.open_services(),
            KeyCode::Char('?') => app.view = View::Help,
            _ => {}
        },
        View::Detail => match key.code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Esc | KeyCode::Char('h') => app.view = View::List,
            KeyCode::Char('r') => {
                let filter = app.selection_request()?;
                return request(app, filter);
            }
            KeyCode::Char('?') => app.view = View::Help,
            _ => {}
        },
        View::Services => match key.code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Char('j') | KeyCode::Down => app.move_services_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => app.move_services_cursor(-1),
            KeyCode::Char('g') | KeyCode::Home => app.move_services_cursor(isize::MIN),
            KeyCode::Char('G') | KeyCode::End => app.move_services_cursor(isize::MAX),
            KeyCode::Char(' ') | KeyCode::Char('x') => {
                let filter = app.toggle_visible();
                return from_menu(app, filter);
            }
            KeyCode::Char('a') | KeyCode::Enter => app.start_entry(),
            KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('h') => app.close_services(),
            _ => {}
        },
        View::Preferences => match key.code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Char('j') | KeyCode::Down => app.move_theme(1),
            KeyCode::Char('k') | KeyCode::Up => app.move_theme(-1),
            KeyCode::Char('b') | KeyCode::Right | KeyCode::Left | KeyCode::Tab => app.cycle_bar(),
            KeyCode::Char('l') => app.cycle_language(),
            KeyCode::Enter => app.save_preferences(),
            KeyCode::Esc | KeyCode::Char('p') | KeyCode::Char('h') => app.cancel_preferences(),
            _ => {}
        },
        View::Help => match key.code {
            KeyCode::Char('q') => app.quit = true,
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Char('?') => app.view = View::List,
            _ => {}
        },
    }
    None
}

/// The termination signals the TUI handles.
///
/// Without this, a `SIGTERM`/`SIGHUP` would kill the process in the middle of
/// raw mode and leave the terminal broken for the shell — the
/// `TerminalGuard`'s `Drop` does not run when a signal kills the process. With
/// `signal-hook`:
///
/// - the **first** signal only sets [`Signals::terminate`] and the loop exits
///   through the normal path, restoring the terminal;
/// - the **second** one exits with code 1 without waiting for the loop, so a
///   repeated `kill` (or a stuck loop) does not leave the process hanging.
struct Signals {
    /// Set when `SIGTERM` or `SIGHUP` arrived.
    terminate: Arc<AtomicBool>,
    /// Which one — the signal number —, or `0` if none arrived yet.
    signal: Arc<AtomicUsize>,
}

impl Signals {
    /// Registers the `SIGTERM` and `SIGHUP` handlers.
    ///
    /// An error here is **not fatal**: it is reported on one line and the TUI
    /// carries on without the handler that failed. Refusing to open the
    /// screen because of it would be worse.
    fn register() -> Signals {
        let terminate = Arc::new(AtomicBool::new(false));
        let signal = Arc::new(AtomicUsize::new(0));
        for number in [SIGTERM, SIGHUP] {
            // The order is the one the `signal-hook` documentation asks for:
            // the conditional shutdown first (it is what looks at the flag),
            // the flag last. The signal number is recorded in between, so
            // whoever sees `terminate` set already knows which signal it was.
            let result =
                signal_hook::flag::register_conditional_shutdown(number, 1, Arc::clone(&terminate))
                    .and_then(|_| {
                        signal_hook::flag::register_usize(
                            number,
                            Arc::clone(&signal),
                            number as usize,
                        )
                    })
                    .and_then(|_| signal_hook::flag::register(number, Arc::clone(&terminate)));
            if let Err(error) = result {
                eprintln!(
                    "{}",
                    t!("tui.run.signal_failed", signal = number, error = error)
                );
            }
        }
        Signals { terminate, signal }
    }

    /// The exit code caused by a signal, if one arrived: `128 + signal` (143
    /// for `SIGTERM`, 129 for `SIGHUP`), as `sh` does.
    fn exit_code(&self) -> Option<i32> {
        if !self.terminate.load(Ordering::SeqCst) {
            return None;
        }
        let signal = self.signal.load(Ordering::SeqCst);
        Some(128 + if signal == 0 { SIGTERM } else { signal as i32 })
    }
}

/// The terminal in raw mode and on the alternate screen.
///
/// Restored on `Drop` — the normal path, and `Ctrl+C` (which the loop treats
/// as an exit) — and on a panic, through the single hook in
/// [`crate::panic_hook`], installed at startup: if the panic is a bug in the
/// program, the terminal goes back to the shell before the message appears; if
/// it is a provider panic, the hook stays quiet and there is nothing to
/// restore here.
struct TerminalGuard;

impl TerminalGuard {
    fn install() -> std::io::Result<TerminalGuard> {
        enable_raw_mode()?;
        // From here on there is a terminal to restore: this is what the panic
        // hook checks before writing to stderr.
        panic_hook::mark_raw_terminal(true);
        if let Err(error) = execute!(std::io::stdout(), EnterAlternateScreen, Hide) {
            // Without the alternate screen, staying in raw mode would still
            // break the shell: undo it before returning the error.
            let _ = panic_hook::restore_terminal();
            return Err(error);
        }
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = panic_hook::restore_terminal();
    }
}
