//! Where a settled turn goes, and why it is never drawn again.
//!
//! `ratatui`'s [`Terminal::insert_before`] emits a block of lines *above* the
//! viewport, into the terminal's own scrollback. Once emitted it belongs to the
//! terminal: selection works, the scrollwheel works, and nothing here can or
//! should repaint it. That is the whole hybrid model (§6.4) in one call.
//!
//! [`Terminal::insert_before`]: ratatui::Terminal::insert_before

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Somewhere a settled turn can be printed once.
///
/// A trait rather than a concrete terminal so the scrollback tests can assert
/// what was emitted without a tty — and so nothing in this crate can quietly
/// start redrawing history, because the only method takes a fresh buffer.
pub trait Scrollback {
    /// How wide the emitted block may be.
    fn width(&self) -> u16;

    /// Print `height` lines above the viewport, drawn by `render`.
    ///
    /// # Errors
    ///
    /// Whatever the terminal said.
    fn insert_before(
        &mut self,
        height: u16,
        render: &mut dyn FnMut(&mut Buffer),
    ) -> std::io::Result<()>;
}

/// A [`Scrollback`] that keeps what it was given.
///
/// The test double, and the only way to assert "printed once": a real terminal
/// forgets, on purpose.
#[derive(Clone, Debug)]
pub struct Recording {
    width: u16,
    blocks: Vec<Vec<String>>,
}

impl Recording {
    /// An empty recorder at a fixed width.
    #[must_use]
    pub fn new(width: u16) -> Self {
        Self {
            width,
            blocks: Vec::new(),
        }
    }

    /// Every block emitted, oldest first.
    #[must_use]
    pub fn blocks(&self) -> &[Vec<String>] {
        &self.blocks
    }

    /// Everything emitted so far, as text.
    #[must_use]
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| block.join("\n"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Scrollback for Recording {
    fn width(&self) -> u16 {
        self.width
    }

    fn insert_before(
        &mut self,
        height: u16,
        render: &mut dyn FnMut(&mut Buffer),
    ) -> std::io::Result<()> {
        let mut buf = Buffer::empty(Rect::new(0, 0, self.width, height.max(1)));
        render(&mut buf);
        self.blocks.push(lines_of(&buf));
        Ok(())
    }
}

/// A buffer's rows as trimmed strings. The snapshot format for every test here.
#[must_use]
pub fn lines_of(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            let mut line = String::new();
            for x in 0..buf.area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            line.trim_end().to_owned()
        })
        .collect()
}
