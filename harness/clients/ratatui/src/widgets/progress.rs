//! Something taking a while. A bar when it is measurable, a label when it is not.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Surface, SurfaceKind};

use crate::theme::Theme;

type Parts<'a> = (&'a String, Option<u64>, Option<u64>);

fn parts(surface: &Surface) -> Option<Parts<'_>> {
    match &surface.kind {
        SurfaceKind::Progress { label, done, total } => Some((label, *done, *total)),
        _ => None,
    }
}

/// The one line a progress surface is.
#[must_use]
fn line(label: &str, done: Option<u64>, total: Option<u64>, width: u16) -> String {
    let Some((done, total)) = done.zip(total).filter(|(_, total)| *total > 0) else {
        // Indeterminate: no bar, because a bar that does not move is a lie.
        return super::elide(&format!("⋯ {label}"), width as usize);
    };
    let text = format!("{label} {done}/{total}");
    let bar_width = (width as usize).saturating_sub(text.chars().count() + 3);
    if bar_width < 4 {
        return super::elide(&text, width as usize);
    }
    let filled = (bar_width as u64 * done / total) as usize;
    format!(
        "[{}{}] {text}",
        "#".repeat(filled.min(bar_width)),
        "·".repeat(bar_width - filled.min(bar_width)),
    )
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((label, done, total)) = parts(surface) else {
        return;
    };
    super::put(
        buf,
        area,
        0,
        &line(label, done, total, area.width),
        theme.text(None),
    );
}

/// One line, always.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    u16::from(parts(surface).is_some())
}
