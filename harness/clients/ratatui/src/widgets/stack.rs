//! Other surfaces, arranged. The tool-call shape, among other things.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{StackDir, Surface, SurfaceKind};

use crate::theme::Theme;

type Parts<'a> = (StackDir, Option<&'a String>, bool, &'a Vec<Surface>);

fn parts(surface: &Surface) -> Option<Parts<'_>> {
    match &surface.kind {
        SurfaceKind::Stack {
            dir,
            title,
            collapsed,
            children,
        } => Some((*dir, title.as_ref(), *collapsed, children)),
        _ => None,
    }
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((dir, title, collapsed, children)) = parts(surface) else {
        return;
    };
    let mut top = area.y;
    if let Some(title) = title {
        let glyph = Theme::status_glyph(surface.status);
        let fold = if collapsed { "+" } else { "-" };
        super::put(
            buf,
            area,
            0,
            &super::elide(&format!("{fold} {glyph} {title}"), area.width as usize),
            theme.text(Some(orrery_proto::TextStyle::Emphasis)),
        );
        top += 1;
    }
    if collapsed {
        return;
    }
    let used = top - area.y;
    let rest = Rect {
        x: area.x,
        y: top,
        width: area.width,
        height: area.height.saturating_sub(used),
    };
    if rest.height == 0 || rest.width == 0 {
        return;
    }
    match dir {
        StackDir::Row => {
            // Even columns. A terminal has no measured layout pass, and columns
            // that resize per child make two adjacent stacks disagree.
            let n = u16::try_from(children.len().max(1)).unwrap_or(1);
            let each = rest.width / n;
            if each == 0 {
                return;
            }
            for (i, child) in children.iter().enumerate() {
                let Ok(i) = u16::try_from(i) else { break };
                super::render(
                    child,
                    Rect {
                        x: rest.x + i * each,
                        y: rest.y,
                        width: each,
                        height: rest.height,
                    },
                    buf,
                    theme,
                );
            }
        }
        _ => {
            let mut y = rest.y;
            for child in children {
                let left = rest.height.saturating_sub(y - rest.y);
                if left == 0 {
                    break;
                }
                let want = super::measure(child, rest.width).min(left);
                super::render(
                    child,
                    Rect {
                        x: rest.x,
                        y,
                        width: rest.width,
                        height: want,
                    },
                    buf,
                    theme,
                );
                y += want;
            }
        }
    }
}

/// How many lines it wants: its title plus its children's.
#[must_use]
pub fn measure(surface: &Surface, width: u16) -> u16 {
    let Some((dir, title, collapsed, children)) = parts(surface) else {
        return 0;
    };
    let head = u16::from(title.is_some());
    if collapsed {
        return head.max(1);
    }
    let body = match dir {
        StackDir::Row => children
            .iter()
            .map(|c| super::measure(c, width / u16::try_from(children.len().max(1)).unwrap_or(1)))
            .max()
            .unwrap_or(0),
        _ => children
            .iter()
            .map(|c| super::measure(c, width))
            .fold(0u16, u16::saturating_add),
    };
    head.saturating_add(body).max(1)
}
