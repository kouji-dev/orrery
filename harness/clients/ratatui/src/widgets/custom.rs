//! A custom surface. In the TUI, always the fallback.
//!
//! Open question 1, decided here: **no custom TUI renderers in phase 4.**
//! Allowing them in Rust means dynamically loading a renderer into this process
//! — libloading, an ABI, a version-skew story, and a crash in somebody's
//! extension taking the terminal with it — for a payoff of at most a nicer box
//! in eighty columns. So this widget draws the mandatory `fallback` and nothing
//! else, and names the kind it is standing in for, which puts a lazy fallback in
//! front of the reader rather than hiding it.
//!
//! The hook point, if an extension ever asks: a map from `kind` to a draw
//! closure, consulted right here and falling through to this code on a miss.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Surface, SurfaceKind};

use crate::theme::Theme;

fn parts(surface: &Surface) -> Option<(&String, &Surface)> {
    match &surface.kind {
        SurfaceKind::Custom { kind, fallback, .. } => Some((kind, fallback)),
        _ => None,
    }
}

/// Draw the fallback.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((kind, fallback)) = parts(surface) else {
        return;
    };
    super::put(
        buf,
        area,
        0,
        &super::elide(&format!("<{kind}>"), area.width as usize),
        theme.muted(),
    );
    if area.height > 1 {
        super::render(
            fallback,
            Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: area.height - 1,
            },
            buf,
            theme,
        );
    }
}

/// The kind line plus whatever the fallback wants.
#[must_use]
pub fn measure(surface: &Surface, width: u16) -> u16 {
    let Some((_, fallback)) = parts(surface) else {
        return 0;
    };
    super::measure(fallback, width).saturating_add(1)
}
