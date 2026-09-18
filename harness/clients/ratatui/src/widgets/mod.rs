//! One widget per core surface, and the dispatcher that must know them all.
//!
//! Every widget is a pure function of a [`Surface`], a [`Rect`] and a
//! [`Theme`] — no session, no runtime, no I/O — so each one is snapshot-tested
//! against a `TestBackend` on its own.
//!
//! # The exhaustiveness rule, and what it actually buys
//!
//! [`SurfaceKind`] is `#[non_exhaustive]`, so a match on it *outside*
//! `orrery-proto` is required by the compiler to carry a `_` arm: a downstream
//! crate cannot spell an exhaustive match, and no attribute here can make it.
//! The plan asked for "adding a variant breaks the build"; the closest true
//! thing is this pair, and both halves are needed:
//!
//! - [`dispatch`] names every variant and its `_` arm is
//!   [`unhandled`], which draws a loud marker rather than nothing;
//! - `conformance::every_core_surface_has_a_widget` reads the variant tags out
//!   of `SurfaceKind`'s own JSON schema and fails if one is not in
//!   [`HANDLED`]. A new variant lands in the schema the moment it is declared,
//!   so the test goes red in the same commit that adds it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use orrery_proto::{Surface, SurfaceKind};

use crate::theme::Theme;

pub mod custom;
pub mod diff;
pub mod form;
pub mod markdown;
pub mod progress;
pub mod question;
pub mod stack;
pub mod stream;
pub mod table;
pub mod task;
pub mod text;
pub mod tree;

/// Every `SurfaceKind` tag this renderer draws.
///
/// The tags, not the variants, because the schema is what a test can compare
/// against. Kept in declaration order so a diff reads like the enum.
pub const HANDLED: &[&str] = &[
    "text", "table", "tree", "diff", "progress", "stream", "task", "question", "form", "stack",
    "markdown", "custom",
];

/// Draw a surface.
///
/// The one entry point: nothing else in this crate matches on [`SurfaceKind`],
/// so there is exactly one place to add a variant.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    dispatch(surface, area, buf, theme);
}

fn dispatch(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    match &surface.kind {
        SurfaceKind::Text { .. } => text::render(surface, area, buf, theme),
        SurfaceKind::Table { .. } => table::render(surface, area, buf, theme),
        SurfaceKind::Tree { .. } => tree::render(surface, area, buf, theme),
        SurfaceKind::Diff { .. } => diff::render(surface, area, buf, theme),
        SurfaceKind::Progress { .. } => progress::render(surface, area, buf, theme),
        SurfaceKind::Stream { .. } => stream::render(surface, area, buf, theme),
        SurfaceKind::Task { .. } => task::render(surface, area, buf, theme),
        SurfaceKind::Question { .. } => question::render(surface, area, buf, theme),
        SurfaceKind::Form { .. } => form::render(surface, area, buf, theme),
        SurfaceKind::Stack { .. } => stack::render(surface, area, buf, theme),
        SurfaceKind::Markdown { .. } => markdown::render(surface, area, buf, theme),
        SurfaceKind::Custom { .. } => custom::render(surface, area, buf, theme),
        // Required by `#[non_exhaustive]`; see the module docs. Loud, because a
        // blank hole in a transcript is the failure mode this is guarding.
        _ => unhandled(area, buf, theme),
    }
}

/// How tall a surface wants to be at this width.
///
/// Used to size the live region and to tell `insert_before` how many lines a
/// settled turn needs.
#[must_use]
pub fn measure(surface: &Surface, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    match &surface.kind {
        SurfaceKind::Text { .. } => text::measure(surface, width),
        SurfaceKind::Table { .. } => table::measure(surface, width),
        SurfaceKind::Tree { .. } => tree::measure(surface, width),
        SurfaceKind::Diff { .. } => diff::measure(surface, width),
        SurfaceKind::Progress { .. } => progress::measure(surface, width),
        SurfaceKind::Stream { .. } => stream::measure(surface, width),
        SurfaceKind::Task { .. } => task::measure(surface, width),
        SurfaceKind::Question { .. } => question::measure(surface, width),
        SurfaceKind::Form { .. } => form::measure(surface, width),
        SurfaceKind::Stack { .. } => stack::measure(surface, width),
        SurfaceKind::Markdown { .. } => markdown::measure(surface, width),
        SurfaceKind::Custom { .. } => custom::measure(surface, width),
        _ => 1,
    }
}

fn unhandled(area: Rect, buf: &mut Buffer, theme: &Theme) {
    put(
        buf,
        area,
        0,
        "(a surface this build has no widget for)",
        theme.text(Some(orrery_proto::TextStyle::Warning)),
    );
}

/// Write one line at row `row` of `area`, clipped to it.
pub(crate) fn put(buf: &mut Buffer, area: Rect, row: u16, text: &str, style: Style) {
    if row >= area.height {
        return;
    }
    buf.set_stringn(area.x, area.y + row, text, area.width as usize, style);
}

/// Break text into lines no wider than `width`, honouring existing newlines.
///
/// Character counts, not grapheme clusters: a terminal cell is one `char` here,
/// which is wrong for combining marks and is the known limit of this wrapper.
#[must_use]
pub(crate) fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut count = 0usize;
        for word in raw.split_inclusive(' ') {
            let len = word.chars().count();
            if count + len > width && count > 0 {
                out.push(std::mem::take(&mut line).trim_end().to_owned());
                count = 0;
            }
            if len > width {
                // A single word wider than the pane: hard-break it rather than
                // let it disappear off the edge.
                for chunk in chunks(word, width) {
                    if count > 0 {
                        out.push(std::mem::take(&mut line).trim_end().to_owned());
                        count = 0;
                    }
                    out.push(chunk);
                }
                continue;
            }
            line.push_str(word);
            count += len;
        }
        out.push(line.trim_end().to_owned());
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn chunks(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(width)
        .map(|c| c.iter().collect::<String>().trim_end().to_owned())
        .collect()
}

/// Cut a string to `width` cells, with an ellipsis when it did not fit.
#[must_use]
pub(crate) fn elide(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut out: String = text.chars().take(width - 1).collect();
    out.push('…');
    out
}
