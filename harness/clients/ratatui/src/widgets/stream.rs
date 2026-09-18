//! An append-only channel, shown as a bounded tail.
//!
//! # The known gap
//!
//! [`SurfaceKind::Stream`] names a channel and carries no body, so the store has
//! nowhere to put the bytes and this widget has nothing to read them from. The
//! conformance fixture says the same thing in its header. What lives here is the
//! half that is this crate's: [`Tail`], the ring buffer a stream's appends land
//! in, bounded so a `cargo build` cannot cost a gigabyte of scrollback — plus
//! the header that names the channel. When `Stream` grows a body field
//! (an `orrery-proto` change, not a fixture one) the widget reads it and the
//! tail stops being unused.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Surface, SurfaceKind};

use crate::theme::Theme;

/// How many lines of a stream are kept. Beyond this the head is dropped: the
/// end of a build log is the part anybody reads.
pub const TAIL: usize = 200;

/// The last `TAIL` lines of a channel.
#[derive(Clone, Debug)]
pub struct Tail {
    lines: VecDeque<String>,
    partial: String,
    cap: usize,
}

impl Default for Tail {
    fn default() -> Self {
        Self::new(TAIL)
    }
}

impl Tail {
    /// A tail keeping `cap` lines.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            partial: String::new(),
            cap: cap.max(1),
        }
    }

    /// Add a chunk, which may end mid-line.
    pub fn push(&mut self, chunk: &str) {
        for ch in chunk.chars() {
            if ch == '\n' {
                let line = std::mem::take(&mut self.partial);
                self.lines.push_back(line);
                while self.lines.len() > self.cap {
                    self.lines.pop_front();
                }
            } else if ch != '\r' {
                self.partial.push(ch);
            }
        }
    }

    /// The kept lines, oldest first, with any unterminated line last.
    #[must_use]
    pub fn lines(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        if !self.partial.is_empty() {
            out.push(&self.partial);
        }
        out
    }

    /// How many lines it is holding.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len() + usize::from(!self.partial.is_empty())
    }

    /// Whether nothing has arrived.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn parts(surface: &Surface) -> Option<&String> {
    match &surface.kind {
        SurfaceKind::Stream { id } => Some(id),
        _ => None,
    }
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some(id) = parts(surface) else { return };
    let glyph = Theme::status_glyph(surface.status);
    super::put(
        buf,
        area,
        0,
        &super::elide(&format!("{glyph} stream {id}"), area.width as usize),
        theme.muted(),
    );
}

/// One line: the channel's name.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    u16::from(parts(surface).is_some())
}
