//! The `json` Orrery renderer: line-delimited AG-UI events for CI, evals and adapters.
//!
//! It is a renderer like any other — it attaches over a transport, feeds a
//! [`SurfaceStore`] and draws. It just draws in JSON.
//!
//! # It draws everything
//!
//! A human client decides what to show and where: a view binding's `placement`
//! says a surface belongs in a side panel, folded, or not at the top level at
//! all. **The json renderer ignores all of that.** Its reader is a diff, a CI
//! log or an adapter, and a surface that was hidden from a person is exactly the
//! surface a failing test needs to see.
//!
//! # It draws a fallback next to its payload
//!
//! A [`SurfaceKind::Custom`] carries a mandatory fallback so that a client with
//! no renderer for it still shows something true. Nothing checks whether that
//! fallback is any good — a one-word placeholder satisfies the type. So this
//! renderer emits the **rendered fallback** beside the payload on every custom
//! surface, which puts a lazy fallback in front of whoever reads CI (§6.2).
//!
//! Implementation plan: `harness/docs/plans/08-protocol-transport.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::io::Write;

use orrery_agui::Frame;
use orrery_client::{StoreChange, SurfaceStore};
use orrery_proto::{Surface, SurfaceKind};

/// The `t` on a fallback line. Namespaced, so nothing confuses it with an
/// AG-UI event.
pub const FALLBACK_LINE: &str = "orrery.fallback";

/// Line-delimited output.
pub struct JsonRenderer<W: Write> {
    out: W,
    store: SurfaceStore,
}

impl<W: Write> JsonRenderer<W> {
    /// A renderer writing to `out`. Usually stdout.
    pub fn new(out: W) -> Self {
        Self {
            out,
            store: SurfaceStore::new(),
        }
    }

    /// What it has understood so far.
    #[must_use]
    pub fn store(&self) -> &SurfaceStore {
        &self.store
    }

    /// Give the writer back.
    pub fn into_inner(self) -> W {
        self.out
    }

    /// Emit one frame, and any fallback it makes visible.
    ///
    /// # Errors
    ///
    /// Whatever the writer said.
    pub fn emit(&mut self, frame: &Frame) -> std::io::Result<()> {
        let line = serde_json::to_string(frame)?;
        writeln!(self.out, "{line}")?;
        for change in self.store.apply(frame) {
            let StoreChange::SurfaceChanged { turn, surface } = change else {
                continue;
            };
            let Some(view) = self.store.turn(&turn).and_then(|t| t.surface(&surface)) else {
                continue;
            };
            // Every custom surface in the tree, not only one that happens to
            // be at the top. An extension composes — a timeline inside a
            // section, a graph beside a table — so a check on `view.kind`
            // alone means the common case never reaches CI, which is the one
            // thing 6.2 asks of this renderer. Phase 4's release train is the
            // case that found it.
            let mut customs = Vec::new();
            collect_customs(&view.kind, &mut customs);
            for (kind, payload, fallback) in customs {
                let line = serde_json::to_string(&serde_json::json!({
                    "t": FALLBACK_LINE,
                    "seq": frame.seq,
                    "turn": turn,
                    "surface": surface,
                    "kind": kind,
                    "payload": payload,
                    "fallback": fallback,
                    "fallback_text": render_text(fallback),
                }))?;
                writeln!(self.out, "{line}")?;
            }
        }
        Ok(())
    }

    /// Emit a whole scenario or replay.
    ///
    /// # Errors
    ///
    /// Whatever the writer said.
    pub fn emit_all<'a>(
        &mut self,
        frames: impl IntoIterator<Item = &'a Frame>,
    ) -> std::io::Result<()> {
        for frame in frames {
            self.emit(frame)?;
        }
        self.out.flush()
    }
}

/// Every `custom` surface in a tree, outermost first.
///
/// A custom surface's fallback may itself hold one — the fallback for a
/// flamegraph could be a simpler graph — so the walk goes through fallbacks as
/// well as through stacks.
fn collect_customs<'a>(
    kind: &'a SurfaceKind,
    out: &mut Vec<(&'a String, &'a serde_json::Value, &'a Surface)>,
) {
    match kind {
        SurfaceKind::Custom {
            kind: name,
            payload,
            fallback,
        } => {
            out.push((name, payload, fallback));
            collect_customs(&fallback.kind, out);
        }
        SurfaceKind::Stack { children, .. } => {
            for child in children {
                collect_customs(&child.kind, out);
            }
        }
        _ => {}
    }
}

/// Flatten a surface to the text a log can carry.
///
/// Deliberately plain: if the only way a fallback reads as useful is with
/// colour and box-drawing, it is not a fallback.
#[must_use]
pub fn render_text(surface: &Surface) -> String {
    match &surface.kind {
        SurfaceKind::Text { value, .. } | SurfaceKind::Markdown { value, .. } => value.clone(),
        SurfaceKind::Table { columns, rows } => {
            let mut out = columns.join(" | ");
            for row in rows {
                out.push('\n');
                out.push_str(
                    &row.iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
            }
            out
        }
        SurfaceKind::Tree { nodes } => nodes
            .iter()
            .map(|n| n.label.clone())
            .collect::<Vec<_>>()
            .join("\n"),
        SurfaceKind::Diff { path, hunks } => {
            format!("{path} ({} hunks)", hunks.len())
        }
        SurfaceKind::Progress { label, done, total } => match (done, total) {
            (Some(done), Some(total)) => format!("{label} {done}/{total}"),
            _ => label.clone(),
        },
        SurfaceKind::Stream { id } => format!("<stream {id}>"),
        SurfaceKind::Task { items } => items
            .iter()
            .map(|i| format!("[{:?}] {}", i.status, i.label))
            .collect::<Vec<_>>()
            .join("\n"),
        SurfaceKind::Question {
            prompt, choices, ..
        } => {
            let choices = choices
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{prompt} [{choices}]")
        }
        SurfaceKind::Form { fields, submit } => {
            format!("{} fields, submit `{submit}`", fields.len())
        }
        SurfaceKind::Stack {
            title, children, ..
        } => {
            let mut out = title.clone().unwrap_or_default();
            for child in children {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&render_text(child));
            }
            out
        }
        // A custom surface inside a fallback: its own fallback is what a
        // renderer without either would show.
        SurfaceKind::Custom { fallback, .. } => render_text(fallback),
        other => format!("{other:?}"),
    }
}
