//! A hierarchy. Expansion is described by the surface and drawn here.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Surface, SurfaceKind, TreeNode};

use crate::theme::Theme;

fn parts(surface: &Surface) -> Option<&Vec<TreeNode>> {
    match &surface.kind {
        SurfaceKind::Tree { nodes } => Some(nodes),
        _ => None,
    }
}

fn flatten(nodes: &[TreeNode], depth: usize, out: &mut Vec<String>) {
    for node in nodes {
        let glyph = if node.children.is_empty() {
            " "
        } else if node.expanded {
            "▾"
        } else {
            "▸"
        };
        out.push(format!("{}{glyph} {}", "  ".repeat(depth), node.label));
        if node.expanded {
            flatten(&node.children, depth + 1, out);
        }
    }
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some(nodes) = parts(surface) else { return };
    let mut lines = Vec::new();
    flatten(nodes, 0, &mut lines);
    for (row, text) in lines.into_iter().enumerate() {
        let Ok(row) = u16::try_from(row) else { break };
        if row >= area.height {
            break;
        }
        super::put(
            buf,
            area,
            row,
            &super::elide(&text, area.width as usize),
            theme.text(None),
        );
    }
}

/// How many lines it wants: one per visible node.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    let Some(nodes) = parts(surface) else {
        return 0;
    };
    let mut lines = Vec::new();
    flatten(nodes, 0, &mut lines);
    u16::try_from(lines.len()).unwrap_or(u16::MAX)
}
