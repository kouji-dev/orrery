//! The live region: the turn in flight, and nothing else.
//!
//! Everything here is redrawn at the frame budget. Everything in scrollback is
//! not redrawn at all. That split is the whole design (§6.4): a settled turn
//! costs nothing per frame and keeps the terminal's own selection, while the
//! turn being streamed is a normal ratatui viewport.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_client::TurnView;

use crate::testing::as_surface;
use crate::theme::Theme;
use crate::widgets;

/// How the live region is laid out and what it is showing.
#[derive(Clone, Debug, Default)]
pub struct LiveRegion {
    /// Which choice the cursor is on, for the turn's question surface.
    pub selection: widgets::question::Selection,
    /// Which field a degraded form is on.
    pub field: usize,
}

impl LiveRegion {
    /// How many rows the turn's surfaces want at this width.
    #[must_use]
    pub fn measure(turn: Option<&TurnView>, width: u16) -> u16 {
        let Some(turn) = turn else { return 0 };
        turn.surfaces
            .iter()
            .map(|view| widgets::measure(&as_surface(view), width))
            .fold(0u16, u16::saturating_add)
    }

    /// Draw the turn's surfaces, top-aligned, clipped to `area`.
    pub fn render(&self, turn: Option<&TurnView>, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let Some(turn) = turn else { return };
        let mut y = area.y;
        let bottom = area.y.saturating_add(area.height);
        for view in &turn.surfaces {
            if y >= bottom {
                break;
            }
            let surface = as_surface(view);
            let want = widgets::measure(&surface, area.width).min(bottom - y);
            let at = Rect {
                x: area.x,
                y,
                width: area.width,
                height: want,
            };
            match &surface.kind {
                // A question is attributed to whoever asked it. A consent
                // prompt is not a surface at all; it is the client's chrome.
                orrery_proto::SurfaceKind::Question { .. } => widgets::question::render_attributed(
                    &surface,
                    Some(&view.id),
                    &self.selection,
                    at,
                    buf,
                    theme,
                ),
                orrery_proto::SurfaceKind::Form { .. } => {
                    widgets::form::render_at(&surface, self.field, at, buf, theme);
                }
                _ => widgets::render(&surface, at, buf, theme),
            }
            y = y.saturating_add(want);
        }
    }
}
