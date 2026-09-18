//! Multi-line input, history, and the control keys.
//!
//! It never talks to the kernel. It produces [`Outgoing`] and the loop hands
//! those to the session, which owns `seq` and the pending queue — so a dropped
//! connection resumes the turn instead of losing it.
//!
//! # Paste
//!
//! Bracketed paste is enabled by the terminal setup, so a pasted block arrives
//! as one [`KeyCode`] run between `Paste` brackets rather than as keystrokes
//! ending in Enter. Without it, pasting a three-line snippet submits the first
//! line and leaves two lines of orphan text in the composer; with it, the whole
//! block lands in the buffer and the person presses Enter when they mean to.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::event::Outgoing;
use crate::theme::Theme;

/// How the composer is drawn and what it holds.
#[derive(Clone, Debug, Default)]
pub struct Composer {
    text: String,
    history: Vec<String>,
    /// `None` means "editing a fresh line", `Some(n)` means history entry `n`.
    browsing: Option<usize>,
    /// What was being typed before the person walked into history.
    stashed: String,
}

/// What a key did to the composer.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Nothing the loop needs to know about.
    None,
    /// Redraw everything: `^L`.
    Redraw,
    /// Send this.
    Send(Outgoing),
}

impl Composer {
    /// An empty composer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What is in it.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Every line submitted so far, oldest first.
    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Take a pasted block. One buffer, whatever newlines are in it.
    ///
    /// Newlines are kept rather than treated as submits: that is the whole
    /// point of bracketed paste.
    pub fn paste(&mut self, block: &str) {
        self.text.push_str(block);
    }

    /// Feed one key.
    ///
    /// `in_turn` says whether a turn is running, because `^C` means two
    /// different things: cancel the turn, or clear the line.
    pub fn key(&mut self, key: KeyEvent, in_turn: bool) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => {
                if in_turn {
                    // Cancel, never exit. `^C` in a coding agent means "stop
                    // what you are doing", and a client that exited instead
                    // would throw away the transcript to stop a `cargo build`.
                    return Action::Send(Outgoing::Cancel);
                }
                self.text.clear();
                Action::None
            }
            KeyCode::Char('d') if ctrl => Action::Send(Outgoing::Exit),
            KeyCode::Char('l') if ctrl => Action::Redraw,
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                Action::None
            }
            KeyCode::Char(c) => {
                self.text.push(c);
                Action::None
            }
            KeyCode::Backspace => {
                self.text.pop();
                Action::None
            }
            // Shift+Enter, or Alt+Enter: a newline, not a submit.
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.text.push('\n');
                Action::None
            }
            KeyCode::Enter => {
                let text = std::mem::take(&mut self.text);
                self.browsing = None;
                if text.trim().is_empty() {
                    return Action::None;
                }
                self.history.push(text.clone());
                Action::Send(Outgoing::Submit(text))
            }
            KeyCode::Up => {
                self.walk(true);
                Action::None
            }
            KeyCode::Down => {
                self.walk(false);
                Action::None
            }
            _ => Action::None,
        }
    }

    fn walk(&mut self, back: bool) {
        if self.history.is_empty() {
            return;
        }
        let next = match (self.browsing, back) {
            (None, true) => {
                self.stashed = self.text.clone();
                Some(self.history.len() - 1)
            }
            (Some(0), true) => Some(0),
            (Some(n), true) => Some(n - 1),
            (Some(n), false) if n + 1 < self.history.len() => Some(n + 1),
            (Some(_), false) => None,
            (None, false) => None,
        };
        self.browsing = next;
        self.text = match next {
            Some(n) => self.history[n].clone(),
            None => std::mem::take(&mut self.stashed),
        };
    }

    /// How many rows it needs.
    #[must_use]
    pub fn height(&self) -> u16 {
        u16::try_from(self.text.split('\n').count())
            .unwrap_or(1)
            .max(1)
    }

    /// Draw it.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        for (row, line) in self.text.split('\n').enumerate() {
            let Ok(row) = u16::try_from(row) else { break };
            if row >= area.height {
                break;
            }
            let prefix = if row == 0 { "> " } else { "  " };
            crate::widgets::put(buf, area, row, &format!("{prefix}{line}"), theme.text(None));
        }
    }
}
