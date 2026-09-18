//! A question, and — separately — a consent prompt.
//!
//! §6.2 is explicit that these are not the same thing, so they do not look the
//! same thing. A **question** is an extension's: it is attributed to whoever
//! asked, drawn inline with the rest of that turn's surfaces, and answering it
//! sends an `intent`. A **consent prompt** is the client's own chrome: minted by
//! the policy engine, drawn in a bordered bar of its own above the footer,
//! carrying a deadline. Nothing an extension emits can produce that bar — which
//! is the point, because an extension that could draw its own consent prompt
//! could phish for a `yes`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Choice, Surface, SurfaceKind};

use crate::theme::Theme;

/// Which choice the cursor is on, and what has been picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// The cursor.
    pub at: usize,
    /// For `multi`, every value ticked.
    pub picked: Vec<String>,
}

type Parts<'a> = (&'a String, &'a Vec<Choice>, bool, Option<&'a String>);

fn parts(surface: &Surface) -> Option<Parts<'_>> {
    match &surface.kind {
        SurfaceKind::Question {
            prompt,
            choices,
            multi,
            default,
            ..
        } => Some((prompt, choices, *multi, default.as_ref())),
        _ => None,
    }
}

/// Draw it, with no attribution and the cursor at the top.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    render_attributed(surface, None, &Selection::default(), area, buf, theme);
}

/// Draw it, attributed to whoever asked.
pub fn render_attributed(
    surface: &Surface,
    asked_by: Option<&str>,
    selection: &Selection,
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
) {
    let Some((prompt, choices, multi, default)) = parts(surface) else {
        return;
    };
    let mut row = 0u16;
    if let Some(who) = asked_by {
        super::put(
            buf,
            area,
            row,
            &super::elide(&format!("? asked by {who}"), area.width as usize),
            theme.muted(),
        );
        row += 1;
    }
    super::put(
        buf,
        area,
        row,
        &super::elide(prompt, area.width as usize),
        theme.text(Some(orrery_proto::TextStyle::Emphasis)),
    );
    row += 1;
    for (n, choice) in choices.iter().enumerate() {
        if row >= area.height {
            break;
        }
        let cursor = if n == selection.at { ">" } else { " " };
        let ticked = if multi {
            if selection.picked.contains(&choice.value) {
                "[x] "
            } else {
                "[ ] "
            }
        } else {
            ""
        };
        let marker = if default == Some(&choice.value) {
            " (default)"
        } else {
            ""
        };
        super::put(
            buf,
            area,
            row,
            &super::elide(
                &format!("{cursor} {ticked}{}{marker}", choice.label),
                area.width as usize,
            ),
            theme.text(None),
        );
        row += 1;
    }
}

/// The attribution line, the prompt and one line per choice.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    let Some((_, choices, _, _)) = parts(surface) else {
        return 0;
    };
    u16::try_from(choices.len() + 2).unwrap_or(u16::MAX)
}

/// The answer a selection becomes on the wire.
#[must_use]
pub fn answer(surface: &Surface, selection: &Selection) -> Option<serde_json::Value> {
    let (_, choices, multi, _) = parts(surface)?;
    if multi {
        return Some(serde_json::json!({ "choices": selection.picked }));
    }
    let choice = choices.get(selection.at)?;
    Some(serde_json::json!({ "choice": choice.value }))
}

/// How many rows [`consent_bar`] needs.
pub const CONSENT_HEIGHT: u16 = 5;

/// Draw the client's own consent bar.
///
/// `remaining_ms` is what is left of the deadline. At zero the bar says so and
/// stops offering keys: the kernel has already resolved the prompt to its
/// fallback (plan 08's translation #12), so an answer typed now would be a lie
/// about what happened.
pub fn consent_bar(
    prompt: &orrery_client::PromptView,
    remaining_ms: u64,
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
) {
    let width = area.width as usize;
    let rule = "─".repeat(width.saturating_sub(11).max(1));
    super::put(buf, area, 0, &format!("┌ consent {rule}┐"), theme.chrome());
    super::put(
        buf,
        area,
        1,
        &super::elide(&format!("│ rule     {}", prompt.reason), width),
        theme.text(None),
    );
    super::put(
        buf,
        area,
        2,
        &super::elide("│ asked by the policy engine, not an extension", width),
        theme.muted(),
    );
    let expired = remaining_ms == 0 || prompt.resolved.is_some();
    let keys = if expired {
        match &prompt.resolved {
            Some(resolution) => format!(
                "│ closed: {:?} by {}",
                resolution.answer, resolution.by
            ),
            None => "│ the deadline passed; the kernel used the fallback".to_owned(),
        }
    } else {
        format!(
            "│ [a] allow once  [s] always  [d] deny{:>width$}",
            format!("{}s ", remaining_ms.div_ceil(1000)),
            width = width.saturating_sub(38).max(1),
        )
    };
    super::put(buf, area, 3, &super::elide(&keys, width), theme.text(None));
    super::put(
        buf,
        area,
        4,
        &format!("└{}┘", "─".repeat(width.saturating_sub(2).max(1))),
        theme.chrome(),
    );
}

/// Whether this bar still takes an answer.
#[must_use]
pub fn accepts_answer(prompt: &orrery_client::PromptView, remaining_ms: u64) -> bool {
    remaining_ms > 0 && prompt.resolved.is_none()
}
