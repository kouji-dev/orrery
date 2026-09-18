//! A small, independent palette.
//!
//! Open question 2 is decided here: the TUI does **not** read the ADE's CSS
//! tokens. A browser has 24-bit colour and a known background; a terminal has
//! sixteen colours it does not control and a background it cannot read. Sharing
//! the token file would mean sharing a contrast model that only one of the two
//! can honour, so the palette is its own, small and named after roles rather
//! than colours.

use ratatui::style::{Color, Modifier, Style};

use orrery_proto::{DiffLineKind, Status, TextStyle};

/// Which colours a terminal will honour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// `false` on a terminal that will not colour: every widget then has to say
    /// the same thing with glyphs. Not a fallback bolted on later — the
    /// degradation snapshots are taken with this off.
    pub colour: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::colour()
    }
}

impl Theme {
    /// The ordinary terminal.
    #[must_use]
    pub const fn colour() -> Self {
        Self { colour: true }
    }

    /// A pipe, a CI log, a terminal with `NO_COLOR` set.
    #[must_use]
    pub const fn monochrome() -> Self {
        Self { colour: false }
    }

    fn paint(self, style: Style) -> Style {
        if self.colour { style } else { Style::default() }
    }

    /// How a run of text is meant.
    #[must_use]
    pub fn text(self, style: Option<TextStyle>) -> Style {
        let base = match style.unwrap_or(TextStyle::Plain) {
            TextStyle::Plain => Style::default(),
            TextStyle::Muted => Style::default().fg(Color::DarkGray),
            TextStyle::Emphasis => Style::default().add_modifier(Modifier::BOLD),
            TextStyle::Code => Style::default().fg(Color::Cyan),
            TextStyle::Error => Style::default().fg(Color::Red),
            TextStyle::Success => Style::default().fg(Color::Green),
            TextStyle::Warning => Style::default().fg(Color::Yellow),
            _ => Style::default(),
        };
        // Emphasis is a weight, not a colour, so it survives monochrome.
        if !self.colour && style == Some(TextStyle::Emphasis) {
            return Style::default().add_modifier(Modifier::BOLD);
        }
        self.paint(base)
    }

    /// A diff line's colour. The marker is the widget's, always drawn.
    #[must_use]
    pub fn diff(self, kind: DiffLineKind) -> Style {
        self.paint(match kind {
            DiffLineKind::Add => Style::default().fg(Color::Green),
            DiffLineKind::Remove => Style::default().fg(Color::Red),
            DiffLineKind::Context => Style::default().fg(Color::Gray),
            _ => Style::default(),
        })
    }

    /// Secondary chrome: headers, rules, counts.
    #[must_use]
    pub fn muted(self) -> Style {
        self.paint(Style::default().fg(Color::DarkGray))
    }

    /// The client's own chrome, which a surface may never borrow.
    #[must_use]
    pub fn chrome(self) -> Style {
        self.paint(Style::default().fg(Color::Magenta))
    }

    /// The marker that stands in for a status when there is no colour.
    #[must_use]
    pub fn status_glyph(status: Option<Status>) -> &'static str {
        match status {
            Some(Status::Pending) => "·",
            Some(Status::Running) => "▸",
            Some(Status::Done) => "✓",
            Some(Status::Failed) => "✗",
            Some(Status::Cancelled) => "⨯",
            _ => " ",
        }
    }
}
