//! Several values at once — or, when there are too many, one at a time.
//!
//! Open question 4, decided here and cross-referenced from plan 09's open
//! question 2: **both, on a field count**. A short form fits on one screen and
//! reads better there; a long one does not fit at all, and half a form is worse
//! than a prompt. The threshold is [`INLINE_MAX`]; above it the widget degrades
//! to the documented sequential prompt, one field at a time, and says which
//! field of how many it is on so the reader knows the shape of what is coming.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Field, FieldKind, Surface, SurfaceKind};

use crate::theme::Theme;

/// The most fields drawn at once. Above this, sequential prompts.
pub const INLINE_MAX: usize = 4;

fn parts(surface: &Surface) -> Option<(&Vec<Field>, &String)> {
    match &surface.kind {
        SurfaceKind::Form { fields, submit } => Some((fields, submit)),
        _ => None,
    }
}

/// What a field's value looks like before anybody has typed.
fn shown(field: &Field) -> String {
    match &field.kind {
        // A secret is never echoed, not even as its default.
        FieldKind::Secret {} => "••••••".to_owned(),
        FieldKind::Bool {} => {
            if field.default.as_deref() == Some("true") {
                "[x]".to_owned()
            } else {
                "[ ]".to_owned()
            }
        }
        FieldKind::Choice { choices } => choices
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>()
            .join(" / "),
        _ => field.default.clone().unwrap_or_default(),
    }
}

fn row(field: &Field, width: usize) -> String {
    let required = if field.required { "*" } else { " " };
    super::elide(
        &format!("{required} {:<12} {}", field.label, shown(field)),
        width,
    )
}

/// Draw it, at the first field.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    render_at(surface, 0, area, buf, theme);
}

/// Draw it, with `at` naming the field the sequential form is on.
pub fn render_at(surface: &Surface, at: usize, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((fields, submit)) = parts(surface) else {
        return;
    };
    let width = area.width as usize;
    if fields.len() > INLINE_MAX {
        let at = at.min(fields.len().saturating_sub(1));
        super::put(
            buf,
            area,
            0,
            &super::elide(&format!("field {} of {}", at + 1, fields.len()), width),
            theme.muted(),
        );
        if let Some(field) = fields.get(at) {
            super::put(buf, area, 1, &row(field, width), theme.text(None));
        }
        super::put(
            buf,
            area,
            2,
            &super::elide(
                &format!("enter: next, last field submits `{submit}`"),
                width,
            ),
            theme.muted(),
        );
        return;
    }
    for (n, field) in fields.iter().enumerate() {
        let Ok(at) = u16::try_from(n) else { break };
        super::put(buf, area, at, &row(field, width), theme.text(None));
    }
    let Ok(last) = u16::try_from(fields.len()) else {
        return;
    };
    super::put(
        buf,
        area,
        last,
        &super::elide(&format!("[ {submit} ]"), width),
        theme.text(Some(orrery_proto::TextStyle::Emphasis)),
    );
}

/// How many lines it wants, which depends on which layout it degraded to.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    let Some((fields, _)) = parts(surface) else {
        return 0;
    };
    if fields.len() > INLINE_MAX {
        return 3;
    }
    u16::try_from(fields.len() + 1).unwrap_or(u16::MAX)
}
