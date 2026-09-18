//! A checklist, with the active item pinned.
//!
//! A plan is usually longer than the live region. Truncating from the bottom
//! hides exactly the item the agent is working on, so when the list does not
//! fit, the running item is kept and the list is windowed around it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Status, Surface, SurfaceKind, TaskItem};

use crate::theme::Theme;

fn parts(surface: &Surface) -> Option<&Vec<TaskItem>> {
    match &surface.kind {
        SurfaceKind::Task { items } => Some(items),
        _ => None,
    }
}

/// Where the window starts so that the active item is inside it.
fn window(items: &[TaskItem], height: usize) -> usize {
    if items.len() <= height {
        return 0;
    }
    let active = items
        .iter()
        .position(|i| i.status == Status::Running)
        .unwrap_or(0);
    active
        .saturating_sub(height / 2)
        .min(items.len() - height)
}

fn style(theme: &Theme, status: Status) -> ratatui::style::Style {
    theme.text(Some(match status {
        Status::Done => orrery_proto::TextStyle::Success,
        Status::Failed => orrery_proto::TextStyle::Error,
        Status::Cancelled => orrery_proto::TextStyle::Muted,
        Status::Running => orrery_proto::TextStyle::Emphasis,
        _ => orrery_proto::TextStyle::Plain,
    }))
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some(items) = parts(surface) else { return };
    let height = area.height as usize;
    let from = window(items, height);
    for (row, item) in items.iter().skip(from).take(height).enumerate() {
        let Ok(row) = u16::try_from(row) else { break };
        let glyph = Theme::status_glyph(Some(item.status));
        super::put(
            buf,
            area,
            row,
            &super::elide(
                &format!("{glyph} {}", item.label),
                area.width as usize,
            ),
            style(theme, item.status),
        );
    }
}

/// One line per item.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    let Some(items) = parts(surface) else { return 0 };
    u16::try_from(items.len()).unwrap_or(u16::MAX)
}
