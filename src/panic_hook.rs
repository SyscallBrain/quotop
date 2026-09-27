//! The program's single panic hook.
//!
//! A `panic!` kills the thread it runs on and, with the default hook, prints
//! the payload to *stderr* — which in the TUI is in raw mode and on the
//! alternate screen, so the message lands on top of a screen that the TUI's
//! terminal guard has not restored yet. The hook installed here solves both:
//!
//! - **inside a provider's zone** ([`crate::engine::in_provider`]) it does
//!   nothing: there a panic is not a bug in the program but a bad response from
//!   a service, and the engine's `catch_unwind` already turns it into
//!   [`crate::model::Status::UnexpectedFormat`]. The payload may contain a
//!   value, so it is neither read nor printed;
//! - **outside it** the panic is our bug: if the terminal is in raw mode, it is
//!   restored before the previous hook prints the message, which must be
//!   visible.
//!
//! `std` has only one hook per process, and it is global: it is installed
//! once, at startup, for both modes (TUI and `--json`). The one that enters
//! raw mode is [`crate::tui`], which reports that state here.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};

use crate::engine;

/// Whether the terminal is in raw mode and on the alternate screen.
///
/// Whoever enters that state sets it to `true` and restoring sets it back to
/// `false`. It exists so that the hook does not write to the terminal when
/// there is nothing to restore: `--json` never uses raw mode.
static RAW_TERMINAL: AtomicBool = AtomicBool::new(false);

/// Reports that the terminal entered (`true`) or left (`false`) raw mode.
pub fn mark_raw_terminal(raw: bool) {
    RAW_TERMINAL.store(raw, Ordering::SeqCst);
}

/// Puts the terminal back as it was: visible cursor, normal screen, cooked
/// mode.
///
/// Idempotent and panic-free: it is called from the terminal guard's `Drop`,
/// from [`crate::tui`]'s error path and from inside a panic hook, where there
/// is nothing better to do with an error than ignore it.
pub fn restore_terminal() -> io::Result<()> {
    RAW_TERMINAL.store(false, Ordering::SeqCst);
    disable_raw_mode()?;
    execute!(io::stdout(), Show, LeaveAlternateScreen)?;
    Ok(())
}

/// Installs the panic hook. Call it **once**, at startup, before choosing the
/// mode (TUI or `--json`).
///
/// The previous hook (the `std` one, or whatever someone else installed) is
/// kept and called outside the provider zone: a panic keeps its normal
/// behaviour wherever it means "bug".
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if engine::in_provider() {
            return;
        }
        if RAW_TERMINAL.load(Ordering::SeqCst) {
            let _ = restore_terminal();
        }
        previous(info);
    }));
}
