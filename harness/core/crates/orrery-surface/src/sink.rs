//! The builders an extension describes surfaces with.
//!
//! `ctx.ui` is an [`orrery_ext_api::SurfaceSink`]. It carries the three
//! builders the extension API crate can define on its own — `text`, `table`,
//! `markdown` — and this trait adds the other nine, so an extension can produce
//! **every** core surface without importing a drawing library, a terminal
//! crate, or anything that knows what a column is.
//!
//! ```
//! use orrery_ext_api::SurfaceSink;
//! use orrery_surface::SurfaceBuilders;
//!
//! # fn example(ui: &SurfaceSink) {
//! ui.table(["crate", "lines"], [["orrery-proto", "2358"]]);
//! ui.progress("linking", Some(3), Some(9));
//! ui.question("Overwrite the file?", [("yes", "Yes"), ("no", "No")]);
//! # }
//! ```
//!
//! # There is no `draw`
//!
//! Every method here returns a [`Surface`] and hands it to the sink. None of
//! them take a terminal, a buffer, a colour or a width: an extension that
//! wanted to draw would have to know which of the five clients it was talking
//! to, and then there would be extensions that only work in one of them. That
//! is the whole bargain, and it is enforced by there being no other method.

use orrery_ext_api::SurfaceSink;
use orrery_proto::{
    Choice, Field, Hunk, StackDir, Status, Surface, SurfaceKind, TaskItem, TreeNode,
};

/// The core surfaces `ctx.ui` can describe, beyond the three on the sink itself.
///
/// An extension trait rather than a second sink type: there is one `ctx.ui` in
/// the world and one place a surface is emitted, and two sinks would mean two.
pub trait SurfaceBuilders {
    /// A hierarchy.
    fn tree(&self, nodes: Vec<TreeNode>) -> Surface;

    /// A change to one file.
    fn diff(&self, path: impl Into<String>, hunks: Vec<Hunk>) -> Surface;

    /// Something taking a while. `done`/`total` absent means indeterminate.
    fn progress(&self, label: impl Into<String>, done: Option<u64>, total: Option<u64>) -> Surface;

    /// An append-only channel: `stdout`, `stderr`, a log name.
    ///
    /// The surface names the channel; the bytes arrive as append patches. A
    /// child process never gets the frame — see the plan's §6.6.
    fn stream(&self, id: impl Into<String>) -> Surface;

    /// A checklist.
    fn task(&self, items: Vec<TaskItem>) -> Surface;

    /// A question the step is blocked on. The answer comes back as an intent.
    fn question<I, V, L>(&self, prompt: impl Into<String>, choices: I) -> Surface
    where
        I: IntoIterator<Item = (V, L)>,
        V: Into<String>,
        L: Into<String>;

    /// A question with a default, a deadline and the free/multi switches.
    fn question_full(
        &self,
        prompt: impl Into<String>,
        choices: Vec<Choice>,
        multi: bool,
        free: bool,
        default: Option<String>,
        deadline_ms: Option<u64>,
    ) -> Surface;

    /// Several values collected at once.
    fn form(&self, fields: Vec<Field>, submit: impl Into<String>) -> Surface;

    /// Other surfaces, arranged.
    fn stack(&self, dir: StackDir, children: Vec<Surface>) -> Surface;

    /// A titled, foldable stack.
    fn section(&self, title: impl Into<String>, collapsed: bool, children: Vec<Surface>)
    -> Surface;

    /// An extension's own surface, and what every other client shows instead.
    ///
    /// `kind` must be `<ext>.<name>`, and the fallback is not optional: a
    /// custom surface nobody can draw is a blank hole in a transcript.
    fn custom(
        &self,
        kind: impl Into<String>,
        payload: serde_json::Value,
        fallback: Surface,
    ) -> Surface;

    /// Give a surface a handle, so a later emission can patch it.
    fn with_status(&self, surface: Surface, status: Status) -> Surface;
}

impl SurfaceBuilders for SurfaceSink {
    fn tree(&self, nodes: Vec<TreeNode>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Tree { nodes }))
    }

    fn diff(&self, path: impl Into<String>, hunks: Vec<Hunk>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Diff {
            path: path.into(),
            hunks,
        }))
    }

    fn progress(&self, label: impl Into<String>, done: Option<u64>, total: Option<u64>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Progress {
            label: label.into(),
            done,
            total,
        }))
    }

    fn stream(&self, id: impl Into<String>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Stream { id: id.into() }))
    }

    fn task(&self, items: Vec<TaskItem>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Task { items }))
    }

    fn question<I, V, L>(&self, prompt: impl Into<String>, choices: I) -> Surface
    where
        I: IntoIterator<Item = (V, L)>,
        V: Into<String>,
        L: Into<String>,
    {
        let choices = choices
            .into_iter()
            .map(|(value, label)| Choice {
                value: value.into(),
                label: label.into(),
            })
            .collect();
        self.question_full(prompt, choices, false, false, None, None)
    }

    fn question_full(
        &self,
        prompt: impl Into<String>,
        choices: Vec<Choice>,
        multi: bool,
        free: bool,
        default: Option<String>,
        deadline_ms: Option<u64>,
    ) -> Surface {
        self.emit(Surface::new(SurfaceKind::Question {
            prompt: prompt.into(),
            choices,
            multi,
            free,
            default,
            deadline_ms,
        }))
    }

    fn form(&self, fields: Vec<Field>, submit: impl Into<String>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Form {
            fields,
            submit: submit.into(),
        }))
    }

    fn stack(&self, dir: StackDir, children: Vec<Surface>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Stack {
            dir,
            title: None,
            collapsed: false,
            children,
        }))
    }

    fn section(
        &self,
        title: impl Into<String>,
        collapsed: bool,
        children: Vec<Surface>,
    ) -> Surface {
        self.emit(Surface::new(SurfaceKind::Stack {
            dir: StackDir::Column,
            title: Some(title.into()),
            collapsed,
            children,
        }))
    }

    fn custom(
        &self,
        kind: impl Into<String>,
        payload: serde_json::Value,
        fallback: Surface,
    ) -> Surface {
        self.emit(Surface::new(SurfaceKind::Custom {
            kind: kind.into(),
            payload,
            fallback: Box::new(fallback),
        }))
    }

    fn with_status(&self, mut surface: Surface, status: Status) -> Surface {
        surface.status = Some(status);
        self.emit(surface)
    }
}

#[cfg(test)]
mod tests {
    use orrery_ext_api::SurfaceSink;
    use orrery_proto::{Field, FieldKind, Hunk, StackDir, Status, SurfaceKind, TaskItem, TreeNode};

    use super::SurfaceBuilders;

    /// The API describes; it has no way to write bytes anywhere.
    ///
    /// Two halves. The types: every builder returns a `Surface` and takes no
    /// terminal, no buffer and no width — if one did, the signature would say
    /// so. And the source: nothing in here reaches for a stream to print to.
    #[test]
    fn describes_never_draws() {
        let (ui, log) = SurfaceSink::recording();

        // All twelve core surfaces, from an extension that imports no drawing
        // library at all.
        ui.text("a run of text");
        ui.markdown("# heading", true);
        ui.table(["crate"], [["orrery-proto"]]);
        ui.tree(vec![TreeNode {
            label: "src".into(),
            id: None,
            expanded: true,
            children: vec![],
        }]);
        ui.diff(
            "src/lib.rs",
            vec![Hunk {
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![],
            }],
        );
        ui.progress("linking", Some(3), Some(9));
        ui.stream("stdout");
        ui.task(vec![TaskItem {
            id: "one".into(),
            label: "read the plan".into(),
            status: Status::Done,
        }]);
        ui.question("Overwrite?", [("yes", "Yes"), ("no", "No")]);
        ui.form(
            vec![Field {
                name: "branch".into(),
                label: "Branch".into(),
                kind: FieldKind::Text {},
                required: true,
                default: None,
            }],
            "Create",
        );
        ui.stack(StackDir::Row, vec![]);
        ui.custom(
            "buildgraph.dag",
            serde_json::json!({ "nodes": 3 }),
            ui.text("proto -> agui -> kernel"),
        );

        let emitted = log.all();
        assert_eq!(
            emitted.len(),
            13,
            "twelve surfaces plus the fallback's text"
        );
        assert!(matches!(emitted[0].kind, SurfaceKind::Text { .. }));
        assert!(matches!(
            emitted.last().unwrap().kind,
            SurfaceKind::Custom { .. }
        ));

        // The source half. A sink that could draw would need one of these.
        let source = include_str!("sink.rs");
        let body = source
            .split("mod tests")
            .next()
            .expect("the implementation, without its own test names");
        for forbidden in [
            "std::io",
            "Stdout",
            "Stderr",
            "print!",
            "eprint!",
            "io::Write",
            "crossterm",
            "ratatui",
            "termcolor",
        ] {
            assert!(
                !body.contains(forbidden),
                "`{forbidden}` in the sink: it describes, it does not draw"
            );
        }
    }
}
