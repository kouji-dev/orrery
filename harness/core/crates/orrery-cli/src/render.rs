//! Draining a hub into a renderer.
//!
//! The two in-binary renderers differ only in what they do with a [`Frame`], so
//! the loop that gets frames out of the kernel is written once here. Neither
//! reaches past the transport: both take frames off a [`ClientStream`], which
//! is the same thing a pipe or an SSE connection carries.
//!
//! Implementation plan: `harness/docs/plans/17-cli.md`

use std::io::Write;

use orrery_agui::{AguiEvent, Frame};
use orrery_client_json::JsonRenderer;
use orrery_client_ratatui::Scrollback;
use orrery_transport::ClientStream;

/// Whether a frame is the end of a run.
#[must_use]
pub fn is_run_finished(frame: &Frame) -> bool {
    matches!(frame.event, AguiEvent::RunFinished { .. })
}

/// Emit frames as line-delimited JSON until the run ends.
///
/// # Errors
///
/// Whatever the writer said.
pub async fn json_until_run_finished<W: Write>(
    stream: &mut ClientStream,
    out: &mut JsonRenderer<W>,
) -> std::io::Result<()> {
    while let Some(batch) = stream.next_batch().await {
        let mut done = false;
        for frame in &batch {
            out.emit(frame)?;
            done |= is_run_finished(frame);
        }
        if done {
            break;
        }
    }
    Ok(())
}

/// A scrollback that keeps what was printed instead of drawing it.
///
/// What `orrery attach --ui ratatui` falls back to when there is no terminal to
/// put into raw mode. The point is not to pretend there is a TUI: it is that a
/// piped `attach` still shows the turn, in the same words the TUI would have
/// used, so a test and a CI log see what a person would.
#[derive(Debug, Default)]
pub struct HeadlessScrollback {
    /// Every line, in the order it was printed.
    pub lines: Vec<String>,
    width: u16,
}

impl HeadlessScrollback {
    /// One this wide.
    #[must_use]
    pub fn new(width: u16) -> Self {
        Self {
            lines: Vec::new(),
            width: width.max(1),
        }
    }
}

impl Scrollback for HeadlessScrollback {
    fn width(&self) -> u16 {
        self.width
    }

    fn insert_before(
        &mut self,
        height: u16,
        render: &mut dyn FnMut(&mut ratatui::buffer::Buffer),
    ) -> std::io::Result<()> {
        let area = ratatui::layout::Rect::new(0, 0, self.width, height.max(1));
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        render(&mut buffer);
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            self.lines.push(line.trim_end().to_owned());
        }
        Ok(())
    }
}
