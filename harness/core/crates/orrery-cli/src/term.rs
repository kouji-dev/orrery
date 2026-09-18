//! Terminal hygiene: raw mode is restored on drop and on panic.
//!
//! A panic in a widget must not leave somebody's shell in raw mode with no
//! echo, and "restore at the end of `run`" does not survive an unwind. Two
//! things together do:
//!
//! - a [`RestoreGuard`](orrery_client_ratatui::RestoreGuard) whose `Drop`
//!   restores, and
//! - a panic hook that restores **before** the default hook prints, so the
//!   backtrace lands in a terminal that can render it.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md` task 10.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this process has put a terminal into raw mode that is still raw.
///
/// Read by the panic hook, which runs on whichever thread panicked and cannot
/// be handed a guard.
static RAW: AtomicBool = AtomicBool::new(false);

/// Install a panic hook that restores the terminal first.
///
/// Idempotent, and cheap when nothing ever enters raw mode: the hook checks
/// [`RAW`] and does nothing when no terminal was touched.
pub fn install_panic_hook() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}

/// Enter the mode the TUI draws in, and remember that we did.
///
/// # Errors
///
/// Whatever the terminal said. A process with no tty fails here, which is the
/// cue to fall back to a headless renderer rather than to give up.
pub fn setup()
-> std::io::Result<ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>> {
    install_panic_hook();
    let terminal = orrery_client_ratatui::terminal::setup()?;
    RAW.store(true, Ordering::SeqCst);
    Ok(terminal)
}

/// Undo [`setup`]. Safe to call twice, and called from a `Drop` and from the
/// panic hook.
pub fn restore() {
    if RAW.swap(false, Ordering::SeqCst) {
        orrery_client_ratatui::terminal::restore();
    }
}

/// Whether this process still holds a terminal in raw mode.
///
/// Only the tests read it today; it is the observable half of the contract
/// this module exists for, so it is public rather than hidden behind a cfg.
#[allow(dead_code)]
#[must_use]
pub fn is_raw() -> bool {
    RAW.load(Ordering::SeqCst)
}

/// A guard that restores on the way out, panic or not.
#[must_use]
pub fn guard() -> orrery_client_ratatui::RestoreGuard<fn()> {
    orrery_client_ratatui::RestoreGuard::new(restore as fn())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panic inside the TUI path leaves the terminal restored.
    ///
    /// The raw-mode *flag* is what is asserted rather than a real tty: a test
    /// process has no terminal to put into raw mode, and the thing that could
    /// break is the bookkeeping — a guard that does not fire, or a panic hook
    /// that never installed. Both are exercised here for real.
    #[test]
    fn panic_restores_the_terminal() {
        install_panic_hook();
        RAW.store(true, Ordering::SeqCst);

        let panicked = std::panic::catch_unwind(|| {
            let _guard = guard();
            panic!("a widget went wrong");
        });

        assert!(panicked.is_err(), "the panic was not swallowed");
        assert!(!is_raw(), "raw mode is off after a panic in the TUI path");
    }

    /// Interrupted before anything was set up, there is nothing to undo — and
    /// undoing it twice is still safe.
    #[test]
    fn restoring_twice_is_fine() {
        restore();
        restore();
        assert!(!is_raw());
    }
}
