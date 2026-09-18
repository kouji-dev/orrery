//! The surface vocabulary: what a tool says it produced, before anybody decides
//! how to draw it.
//!
//! A surface is a *description*, not a rendering. The same `diff` is a side-by-side
//! pane in the ADE, a coloured unified hunk in the terminal client and plain text
//! in a log, and the tool that produced it knows about none of that. Twelve
//! variants cover it; anything else is [`SurfaceKind::Custom`], which must carry a
//! fallback so that a client with no renderer for it still shows something true.
//!
//! Every tagged enum here uses **struct variants**, without exception. Internal
//! tagging cannot be applied to a newtype variant wrapping a non-map, which
//! would break CBOR — see the round-trip test.

use serde::{Deserialize, Serialize};

use crate::ids::SurfaceId;

/// A surface that is not well-formed.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceError {
    /// `custom.kind` was not `<ext>.<name>`.
    #[error(
        "custom surface kind `{kind}` is not namespaced: expected `<ext>.<name>`, \
         so that two extensions cannot claim the same renderer"
    )]
    UnnamespacedCustomKind {
        /// The offending kind.
        kind: String,
    },
}

/// How far along whatever produced this surface is.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Not started.
    Pending,
    /// In flight; more patches are coming.
    Running,
    /// Finished, successfully.
    Done,
    /// Finished, unsuccessfully.
    Failed,
    /// Stopped before it finished.
    Cancelled,
}

/// A hint about how a run of text is meant, never a colour.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextStyle {
    /// Ordinary prose.
    Plain,
    /// Secondary: a count, a path, a timing.
    Muted,
    /// Worth noticing.
    Emphasis,
    /// Monospace: an identifier, a command.
    Code,
    /// Something went wrong.
    Error,
    /// Something went right.
    Success,
    /// Something might go wrong.
    Warning,
}

/// One cell of a [`SurfaceKind::Table`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Cell {
    /// The cell's text, already formatted by whoever knows the units.
    pub text: String,
    /// How it is meant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TextStyle>,
}

/// One node of a [`SurfaceKind::Tree`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TreeNode {
    /// What to show.
    pub label: String,
    /// A stable handle, when the client needs to address this node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Whether the node starts open.
    pub expanded: bool,
    /// Its children. A leaf has none.
    #[serde(default)]
    pub children: Vec<TreeNode>,
}

/// What a line of a diff is.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiffLineKind {
    /// Unchanged.
    Context,
    /// Added.
    Add,
    /// Removed.
    Remove,
}

/// One line inside a [`Hunk`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiffLine {
    /// Added, removed or unchanged.
    pub kind: DiffLineKind,
    /// The line itself, without its leading marker.
    pub text: String,
}

/// One changed region of a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Hunk {
    /// First line of the region in the old file, 1-based.
    pub old_start: u64,
    /// How many lines it covered.
    pub old_lines: u64,
    /// First line of the region in the new file, 1-based.
    pub new_start: u64,
    /// How many lines it covers.
    pub new_lines: u64,
    /// The lines.
    #[serde(default)]
    pub lines: Vec<DiffLine>,
}

/// One entry of a [`SurfaceKind::Task`] list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskItem {
    /// A stable handle, so a patch can update one item.
    pub id: String,
    /// What to show.
    pub label: String,
    /// How far along it is.
    pub status: Status,
}

/// One option of a [`SurfaceKind::Question`] or of a choice [`Field`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Choice {
    /// What is sent back on selection.
    pub value: String,
    /// What is shown.
    pub label: String,
}

/// What a [`Field`] collects.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum FieldKind {
    /// A line of text.
    Text {},
    /// A line of text that must not be echoed, logged or stored.
    Secret {},
    /// A number, collected as text and parsed by whoever declared the field.
    Number {},
    /// A checkbox.
    Bool {},
    /// One of a fixed set.
    Choice {
        /// The options.
        choices: Vec<Choice>,
    },
}

/// One input of a [`SurfaceKind::Form`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Field {
    /// The key this field's value is submitted under.
    pub name: String,
    /// What is shown next to it.
    pub label: String,
    /// What it collects.
    pub kind: FieldKind,
    /// Whether the form can be submitted without it.
    pub required: bool,
    /// What it starts as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Which way a [`SurfaceKind::Stack`] lays its children out.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StackDir {
    /// Left to right.
    Row,
    /// Top to bottom.
    Column,
}

/// What a surface *is*.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum SurfaceKind {
    /// A run of text.
    Text {
        /// The text.
        value: String,
        /// How it is meant.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<TextStyle>,
    },
    /// Rows under headers.
    Table {
        /// The header row.
        columns: Vec<String>,
        /// The body. Every row is expected to be `columns.len()` long.
        #[serde(default)]
        rows: Vec<Vec<Cell>>,
    },
    /// A hierarchy.
    Tree {
        /// The roots.
        #[serde(default)]
        nodes: Vec<TreeNode>,
    },
    /// A change to one file.
    Diff {
        /// The file.
        path: String,
        /// The changed regions.
        #[serde(default)]
        hunks: Vec<Hunk>,
    },
    /// Something taking a while. `done`/`total` absent means indeterminate.
    Progress {
        /// What is happening.
        label: String,
        /// How much is done.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        done: Option<u64>,
        /// How much there is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    /// An append-only channel. The content arrives as
    /// [`SurfacePatch::Append`], which is the hot path.
    Stream {
        /// Which channel: `stdout`, `stderr`, a log name.
        id: String,
    },
    /// A checklist.
    Task {
        /// The entries.
        #[serde(default)]
        items: Vec<TaskItem>,
    },
    /// A question the run is blocked on.
    Question {
        /// What is being asked.
        prompt: String,
        /// The offered answers.
        #[serde(default)]
        choices: Vec<Choice>,
        /// Whether more than one may be picked.
        multi: bool,
        /// Whether an answer outside `choices` is accepted.
        free: bool,
        /// What is used if the deadline passes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        /// How long there is to answer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deadline_ms: Option<u64>,
    },
    /// Several values collected at once.
    Form {
        /// The inputs.
        #[serde(default)]
        fields: Vec<Field>,
        /// The submit button's label.
        submit: String,
    },
    /// Other surfaces, arranged.
    Stack {
        /// Which way.
        dir: StackDir,
        /// An optional heading.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Whether it starts folded.
        collapsed: bool,
        /// The contents.
        #[serde(default)]
        children: Vec<Surface>,
    },
    /// Markdown. `complete` is false while it is still streaming, so a client
    /// knows not to trust a half-open code fence.
    Markdown {
        /// The source.
        value: String,
        /// Whether the last patch has arrived.
        complete: bool,
    },
    /// Anything an extension brought its own renderer for.
    Custom {
        /// `<ext>.<name>`, so two extensions cannot claim one renderer.
        kind: String,
        /// Whatever that renderer needs.
        payload: serde_json::Value,
        /// What to show instead, for a client that has no such renderer.
        ///
        /// Mandatory, and a `Box<Surface>` rather than an `Option`: a custom
        /// surface nobody can draw is a blank hole in a transcript, so the type
        /// refuses to express one.
        fallback: Box<Surface>,
    },
}

impl SurfaceKind {
    /// Check what can be checked without knowing the client.
    ///
    /// Today that is one rule: `custom.kind` must be namespaced. It recurses
    /// into `stack.children` and `custom.fallback`.
    ///
    /// Deliberately *not* checked here: a `question` with a `deadline_ms` and
    /// no `default` is an error only when nobody is attending the session, and
    /// this crate does not know whether anybody is. That rule belongs to the
    /// surface differ (plan 09).
    ///
    /// # Errors
    ///
    /// [`SurfaceError`] describing the first problem found.
    pub fn validate(&self) -> Result<(), SurfaceError> {
        match self {
            SurfaceKind::Custom { kind, fallback, .. } => {
                let namespaced = kind
                    .split_once('.')
                    .is_some_and(|(ext, name)| !ext.is_empty() && !name.is_empty());
                if !namespaced {
                    return Err(SurfaceError::UnnamespacedCustomKind { kind: kind.clone() });
                }
                fallback.validate()
            }
            SurfaceKind::Stack { children, .. } => {
                for child in children {
                    child.validate()?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// A surface, with the handle a patch addresses it by and how far along it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Surface {
    /// The handle. Absent for a surface nothing will ever patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<SurfaceId>,
    /// How far along it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// What it is.
    pub kind: SurfaceKind,
}

impl Surface {
    /// A surface with no id and no status.
    #[must_use]
    pub fn new(kind: SurfaceKind) -> Self {
        Self {
            id: None,
            status: None,
            kind,
        }
    }

    /// See [`SurfaceKind::validate`].
    ///
    /// # Errors
    ///
    /// [`SurfaceError`] describing the first problem found.
    pub fn validate(&self) -> Result<(), SurfaceError> {
        self.kind.validate()
    }
}

/// A change to a surface already on screen.
///
/// The whole point of the surface protocol: a streaming tool sends one
/// [`SurfacePatch::Append`] per chunk rather than a whole new surface, so the
/// hot path is a string and not a tree.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum SurfacePatch {
    /// Swap the whole surface.
    Replace {
        /// Which surface.
        id: SurfaceId,
        /// What it becomes.
        value: Surface,
    },
    /// Add to the end of a stream or a markdown body.
    Append {
        /// Which surface.
        id: SurfaceId,
        /// What to add.
        text: String,
    },
    /// Set one field, addressed by a path of keys.
    Set {
        /// Which surface.
        id: SurfaceId,
        /// The path from the surface's root to the field.
        path: Vec<String>,
        /// The new value.
        value: serde_json::Value,
    },
    /// Take the surface away.
    Remove {
        /// Which surface.
        id: SurfaceId,
    },
}
