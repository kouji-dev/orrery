//! Terminal setup and teardown, and the real [`Scrollback`].
//!
//! # No alternate screen
//!
//! Deliberate, and the single most load-bearing line in this module. The
//! alternate screen is a second buffer with no scrollback: entering it is how a
//! full-screen TUI throws away the transcript the moment it exits. This client
//! stays on the primary screen so that everything it printed is still there
//! afterwards, in the terminal's own history, selectable and greppable.
//!
//! # No mouse capture, ever
//!
//! Open question 3, decided here. `crossterm` can enable mouse reporting, and
//! doing so makes the terminal hand click-and-drag to this process instead of
//! using it to select text. Native selection is the entire reason the hybrid
//! model exists — it is why settled turns go to scrollback rather than into a
//! widget. Capturing the mouse would buy a clickable option list and cost the
//! feature the architecture was designed around, and the workaround terminals
//! offer (hold shift to select) is an undiscoverable tax on every copy.
//!
//! This will be proposed again, because clickable lists demo well. The answer
//! is no: keyboard selection covers the same ground, and [`EnableMouseCapture`]
//! is not imported by this crate on purpose.
//!
//! [`EnableMouseCapture`]: https://docs.rs/crossterm/latest/crossterm/event/struct.EnableMouseCapture.html

use std::io::{Stdout, Write, stdout};

use crossterm::ExecutableCommand;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;

use crate::scrollback::Scrollback;

/// Put the terminal into the mode this client draws in.
///
/// Raw mode and bracketed paste. Not the alternate screen, and not mouse
/// capture; see the module docs for why neither is an oversight.
///
/// # Errors
///
/// Whatever the terminal said.
pub fn setup() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    out.execute(EnableBracketedPaste)?;
    Terminal::new(CrosstermBackend::new(out))
}

/// Undo [`setup`]. Safe to call twice, and called from a `Drop`.
///
/// Errors are swallowed on purpose: this runs while unwinding from a panic, and
/// a second failure there would replace the useful message with a useless one.
pub fn restore() {
    let mut out = stdout();
    let _ = out.execute(DisableBracketedPaste);
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// The real scrollback: `ratatui`'s `insert_before`.
pub struct TerminalScrollback<'a, W: Write> {
    terminal: &'a mut Terminal<CrosstermBackend<W>>,
    width: u16,
}

impl<'a, W: Write> TerminalScrollback<'a, W> {
    /// Wrap a terminal.
    pub fn new(terminal: &'a mut Terminal<CrosstermBackend<W>>, width: u16) -> Self {
        Self { terminal, width }
    }
}

impl<W: Write> Scrollback for TerminalScrollback<'_, W> {
    fn width(&self) -> u16 {
        self.width
    }

    fn insert_before(
        &mut self,
        height: u16,
        render: &mut dyn FnMut(&mut Buffer),
    ) -> std::io::Result<()> {
        self.terminal
            .insert_before(height.max(1), |buf| render(buf))
    }
}
