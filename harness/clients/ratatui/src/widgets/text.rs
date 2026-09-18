//! A run of text, wrapped and styled.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Surface, SurfaceKind};

use crate::theme::Theme;

fn parts(surface: &Surface) -> Option<(&String, Option<orrery_proto::TextStyle>)> {
    match &surface.kind {
        SurfaceKind::Text { value, style } => Some((value, *style)),
        _ => None,
    }
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((value, style)) = parts(surface) else {
        return;
    };
    let style = theme.text(style);
    for (row, line) in super::wrap(value, area.width).into_iter().enumerate() {
        let Ok(row) = u16::try_from(row) else { break };
        if row >= area.height {
            break;
        }
        super::put(buf, area, row, &line, style);
    }
}

/// How many lines it wants.
#[must_use]
pub fn measure(surface: &Surface, width: u16) -> u16 {
    let Some((value, _)) = parts(surface) else {
        return 0;
    };
    u16::try_from(super::wrap(value, width).len()).unwrap_or(u16::MAX)
}
