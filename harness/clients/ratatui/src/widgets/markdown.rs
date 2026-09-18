//! Markdown: plain while it is streaming, formatted once it is complete.
//!
//! §6.5's rule, and the reason for it: mid-stream the source is *not* markdown
//! — a fence may be half open, a table half drawn — so formatting it produces a
//! shape that changes under the reader. Plain text does not lie. The switch is
//! `complete`, which the store sets from `TEXT_MESSAGE_END`, not a guess here.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use orrery_proto::{Surface, SurfaceKind, TextStyle};

use crate::theme::Theme;

/// How wide the rule that stands in for a code fence is drawn.
const FENCE_RULE: u16 = 12;

fn parts(surface: &Surface) -> Option<(&String, bool)> {
    match &surface.kind {
        SurfaceKind::Markdown { value, complete } => Some((value, *complete)),
        _ => None,
    }
}

/// The lines this markdown becomes, with the style each one is drawn in.
fn lines(value: &str, complete: bool, width: u16, theme: &Theme) -> Vec<(String, Style)> {
    if !complete {
        return super::wrap(value, width)
            .into_iter()
            .map(|line| (line, theme.text(None)))
            .collect();
    }
    let mut out = Vec::new();
    let mut in_fence = false;
    for raw in value.split('\n') {
        if raw.trim_start().starts_with("```") {
            in_fence = !in_fence;
            // A rule of a constant width: a fence whose rule is as long as
            // its info string makes an opening fence and a closing one look
            // like different things.
            out.push(("─".repeat(width.min(FENCE_RULE) as usize), theme.muted()));
            continue;
        }
        if in_fence {
            for line in super::wrap(raw, width.saturating_sub(2).max(1)) {
                out.push((format!("  {line}"), theme.text(Some(TextStyle::Code))));
            }
            continue;
        }
        let (body, style) = if let Some(rest) = raw.trim_start().strip_prefix("# ") {
            (
                rest.to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            )
        } else if let Some(rest) = raw.trim_start().strip_prefix("## ") {
            (
                rest.to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            )
        } else if let Some(rest) = raw.trim_start().strip_prefix("- ") {
            (format!("• {rest}"), theme.text(None))
        } else {
            (raw.to_owned(), theme.text(None))
        };
        for line in super::wrap(&body, width) {
            out.push((line, style));
        }
    }
    if out.is_empty() {
        out.push((String::new(), theme.text(None)));
    }
    out
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((value, complete)) = parts(surface) else {
        return;
    };
    for (row, (line, style)) in lines(value, complete, area.width, theme)
        .into_iter()
        .enumerate()
    {
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
    let Some((value, complete)) = parts(surface) else {
        return 0;
    };
    u16::try_from(lines(value, complete, width, &Theme::monochrome()).len()).unwrap_or(u16::MAX)
}
