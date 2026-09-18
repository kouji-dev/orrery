//! A change to one file.
//!
//! The marker is drawn **always**, colour or no colour. A diff that is only
//! legible in colour is a diff that is illegible over ssh, in a pipe, and to a
//! reader who cannot tell red from green — so `+`/`-`/space lead every line and
//! the colour is the second signal, never the only one.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{DiffLineKind, Hunk, Surface, SurfaceKind};

use crate::theme::Theme;

fn parts(surface: &Surface) -> Option<(&String, &Vec<Hunk>)> {
    match &surface.kind {
        SurfaceKind::Diff { path, hunks } => Some((path, hunks)),
        _ => None,
    }
}

/// The marker a line always carries.
#[must_use]
pub fn marker(kind: DiffLineKind) -> char {
    match kind {
        DiffLineKind::Add => '+',
        DiffLineKind::Remove => '-',
        _ => ' ',
    }
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((path, hunks)) = parts(surface) else {
        return;
    };
    let mut row = 0u16;
    let mut put = |row: &mut u16, text: &str, style| {
        if *row < area.height {
            super::put(
                buf,
                area,
                *row,
                &super::elide(text, area.width as usize),
                style,
            );
        }
        *row += 1;
    };
    put(&mut row, &format!("── {path}"), theme.muted());
    for hunk in hunks {
        put(
            &mut row,
            &format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ),
            theme.muted(),
        );
        for line in &hunk.lines {
            put(
                &mut row,
                &format!("{}{}", marker(line.kind), line.text),
                theme.diff(line.kind),
            );
        }
    }
}

/// How many lines it wants: the path, then a header and its lines per hunk.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    let Some((_, hunks)) = parts(surface) else {
        return 0;
    };
    let body: usize = hunks.iter().map(|h| h.lines.len() + 1).sum();
    u16::try_from(body + 1).unwrap_or(u16::MAX)
}
