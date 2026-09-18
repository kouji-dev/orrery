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

/// One run of text inside a line, and how it is drawn.
#[derive(Clone, Debug)]
struct Span {
    text: String,
    style: Style,
}

/// Split one source line into its emphasis runs.
///
/// `**bold**`, `*italic*`, `_italic_` and `` `code` `` become spans with the
/// markers **removed**. Weight and colour are a client's own business (plan 09,
/// open question 1), but the characters are not: a renderer that printed the
/// asterisks would be showing different text from one that did not, and two
/// clients showing different text for one surface is exactly the
/// incomparability that rule forbids. Phase 4 found this — Ink drew `4`, this
/// drew `**4**`.
///
/// Deliberately small: no links, no nesting, no escapes. An unmatched marker is
/// left alone, because half a `**` is not emphasis.
fn inline(raw: &str, base: Style, theme: &Theme) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut rest = raw;
    while !rest.is_empty() {
        let matched = ["**", "`", "*", "_"].into_iter().find_map(|marker| {
            let body = rest.strip_prefix(marker)?;
            let end = body.find(marker)?;
            if end == 0 {
                return None;
            }
            let style = if marker == "`" {
                theme.text(Some(TextStyle::Code))
            } else if marker == "**" {
                base.add_modifier(Modifier::BOLD)
            } else {
                base.add_modifier(Modifier::ITALIC)
            };
            Some((
                Span {
                    text: body[..end].to_owned(),
                    style,
                },
                marker.len() * 2 + end,
            ))
        });
        match matched {
            Some((span, consumed)) => {
                if !plain.is_empty() {
                    out.push(Span {
                        text: std::mem::take(&mut plain),
                        style: base,
                    });
                }
                out.push(span);
                rest = &rest[consumed..];
            }
            None => {
                let ch = rest.chars().next().expect("rest is not empty");
                plain.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    if !plain.is_empty() || out.is_empty() {
        out.push(Span {
            text: plain,
            style: base,
        });
    }
    out
}

/// Greedy wrap over spans, breaking at spaces and keeping each run's style.
fn wrap_spans(spans: &[Span], width: u16) -> Vec<Vec<Span>> {
    let width = width.max(1) as usize;
    let mut rows: Vec<Vec<Span>> = vec![Vec::new()];
    let mut used = 0usize;
    for span in spans {
        for word in span.text.split_inclusive(' ') {
            let len = word.chars().count();
            if used + len > width && used > 0 {
                trim_end(rows.last_mut().expect("a row"));
                rows.push(Vec::new());
                used = 0;
            }
            match rows.last_mut().expect("a row").last_mut() {
                Some(last) if last.style == span.style => last.text.push_str(word),
                _ => rows.last_mut().expect("a row").push(Span {
                    text: word.to_owned(),
                    style: span.style,
                }),
            }
            used += len;
        }
    }
    trim_end(rows.last_mut().expect("a row"));
    rows
}

fn trim_end(row: &mut Vec<Span>) {
    if let Some(last) = row.last_mut() {
        let trimmed = last.text.trim_end().to_owned();
        last.text = trimmed;
        if last.text.is_empty() && row.len() > 1 {
            row.pop();
        }
    }
}

/// The lines this markdown becomes, each as the runs it is drawn from.
fn lines(value: &str, complete: bool, width: u16, theme: &Theme) -> Vec<Vec<Span>> {
    if !complete {
        return super::wrap(value, width)
            .into_iter()
            .map(|line| {
                vec![Span {
                    text: line,
                    style: theme.text(None),
                }]
            })
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
            out.push(vec![Span {
                text: "─".repeat(width.min(FENCE_RULE) as usize),
                style: theme.muted(),
            }]);
            continue;
        }
        if in_fence {
            // A fence is drawn whole and never re-parsed for emphasis: a `*`
            // inside code is a `*`.
            for line in super::wrap(raw, width.saturating_sub(2).max(1)) {
                out.push(vec![Span {
                    text: format!("  {line}"),
                    style: theme.text(Some(TextStyle::Code)),
                }]);
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
        out.extend(wrap_spans(&inline(&body, style, theme), width));
    }
    if out.is_empty() {
        out.push(vec![Span {
            text: String::new(),
            style: theme.text(None),
        }]);
    }
    out
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((value, complete)) = parts(surface) else {
        return;
    };
    for (row, spans) in lines(value, complete, area.width, theme)
        .into_iter()
        .enumerate()
    {
        let Ok(row) = u16::try_from(row) else { break };
        if row >= area.height {
            break;
        }
        let mut x = 0u16;
        for span in spans {
            let taken = u16::try_from(span.text.chars().count()).unwrap_or(u16::MAX);
            let room = area.width.saturating_sub(x);
            if room == 0 {
                break;
            }
            buf.set_stringn(
                area.x + x,
                area.y + row,
                &span.text,
                room as usize,
                span.style,
            );
            x = x.saturating_add(taken.min(room));
        }
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
