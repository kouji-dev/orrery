//! Rows under headers, and what to drop when the pane is narrower than they are.
//!
//! The documented degradation (§6.6) is **elide middle columns**, never wrap a
//! cell: a table that wraps into three ragged lines per row is less readable
//! than one that admits it dropped a column. The first and last columns survive,
//! because they are the ones a reader scans.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_proto::{Cell, Surface, SurfaceKind};

use crate::theme::Theme;

/// What is left of a table once it has been fitted to a width.
struct Fitted {
    /// Indices of the columns that survived, in order.
    keep: Vec<usize>,
    /// The width each surviving column is drawn at.
    widths: Vec<usize>,
    /// Whether anything was dropped.
    elided: bool,
}

const GAP: &str = "  ";

fn parts(surface: &Surface) -> Option<(&Vec<String>, &Vec<Vec<Cell>>)> {
    match &surface.kind {
        SurfaceKind::Table { columns, rows } => Some((columns, rows)),
        _ => None,
    }
}

fn fit(columns: &[String], rows: &[Vec<Cell>], width: u16) -> Fitted {
    let natural: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(i, header)| {
            rows.iter()
                .filter_map(|row| row.get(i))
                .map(|c| c.text.chars().count())
                .chain(std::iter::once(header.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut keep: Vec<usize> = (0..columns.len()).collect();
    let mut elided = false;
    // Drop from the middle outwards until it fits, or until only the outer two
    // columns are left; below that, elide the cells instead.
    while keep.len() > 2 && span(&natural, &keep, elided) > width as usize {
        keep.remove(keep.len() / 2);
        elided = true;
    }
    let mut widths: Vec<usize> = keep.iter().map(|i| natural[*i]).collect();
    let mut total = span(&natural, &keep, elided);
    // Still too wide: shave the widest surviving column.
    while total > width as usize && !widths.is_empty() {
        let Some((at, _)) = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| **w)
            .filter(|(_, w)| **w > 1)
        else {
            break;
        };
        widths[at] -= 1;
        total -= 1;
    }
    Fitted {
        keep,
        widths,
        elided,
    }
}

fn span(natural: &[usize], keep: &[usize], elided: bool) -> usize {
    let cells: usize = keep.iter().map(|i| natural[*i]).sum();
    let gaps = keep.len().saturating_sub(1) * GAP.len();
    cells + gaps + usize::from(elided)
}

fn line(values: &[&str], fitted: &Fitted) -> String {
    let mut out = String::new();
    for (n, (value, width)) in values.iter().zip(&fitted.widths).enumerate() {
        if n > 0 {
            out.push_str(GAP);
            if fitted.elided && n == fitted.widths.len() / 2 {
                out.push('…');
            }
        }
        let cut = super::elide(value, *width);
        out.push_str(&cut);
        for _ in cut.chars().count()..*width {
            out.push(' ');
        }
    }
    out.trim_end().to_owned()
}

/// Draw it.
pub fn render(surface: &Surface, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let Some((columns, rows)) = parts(surface) else {
        return;
    };
    let fitted = fit(columns, rows, area.width);
    let headers: Vec<&str> = fitted.keep.iter().map(|i| columns[*i].as_str()).collect();
    super::put(buf, area, 0, &line(&headers, &fitted), theme.muted());

    for (n, row) in rows.iter().enumerate() {
        let Ok(at) = u16::try_from(n + 1) else { break };
        if at >= area.height {
            break;
        }
        let values: Vec<&str> = fitted
            .keep
            .iter()
            .map(|i| row.get(*i).map_or("", |c| c.text.as_str()))
            .collect();
        // One style for the row, taken from its first styled cell: a terminal
        // row that changes colour mid-way reads as two rows.
        let style = fitted
            .keep
            .iter()
            .find_map(|i| row.get(*i).and_then(|c| c.style))
            .map_or_else(|| theme.text(None), |s| theme.text(Some(s)));
        super::put(buf, area, at, &line(&values, &fitted), style);
    }
}

/// How many lines it wants: the header and every row.
#[must_use]
pub fn measure(surface: &Surface, _width: u16) -> u16 {
    let Some((_, rows)) = parts(surface) else {
        return 0;
    };
    u16::try_from(rows.len() + 1).unwrap_or(u16::MAX)
}
