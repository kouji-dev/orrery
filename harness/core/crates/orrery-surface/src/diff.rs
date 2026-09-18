//! The differ, and the one function that undoes it.
//!
//! The rules, in the order they are tried:
//!
//! 1. Discriminant changed ⇒ `Replace`.
//! 2. `text` / `markdown` where `next.value.starts_with(&prev.value)` ⇒
//!    `Append` with the suffix. This is the hot path: a streaming answer is a
//!    string on the wire, not a tree.
//! 3. `stack` ⇒ match children by explicit `id` first, then by index; recurse.
//! 4. Row and item collections ⇒ diff at row granularity into `Set { path }`.
//! 5. Leaf scalar change ⇒ `Set { path }`.
//! 6. **Cost guard** ⇒ if the ops cost more than [`COST_GUARD`] of a whole
//!    `Replace`, throw them away and send the `Replace`. This is what stops a
//!    re-sorted table producing a hundred `set` ops.
//!
//! # `stream` has no body here
//!
//! [`SurfaceKind::Stream`](orrery_proto::SurfaceKind::Stream) carries a channel
//! name and nothing else: its content arrives as `Append` frames addressed to
//! the surface id, coalesced to a frame budget by the transport. So the differ
//! treats a `stream` as a leaf, and rule 2 covers `text` and `markdown` only.
//!
//! # Paths
//!
//! A `Set`'s path is the JSON path from the surface's root: `["kind", "rows",
//! "3"]`, `["kind", "children", "7", "kind", "value"]`. Array indices are
//! decimal strings. One shape, so [`apply`] is one walk rather than a match arm
//! per field.

use orrery_proto::{Surface, SurfaceId, SurfaceKind, SurfacePatch};
use serde::Serialize;
use serde_json::Value;

use crate::ApplyError;
use crate::hash::{HashTree, hash_tree};

/// Accumulated patch bytes over this fraction of a whole `Replace` ⇒ send the
/// `Replace` instead.
pub const COST_GUARD: f64 = 0.6;

/// Below this, a surface is too small for the guard to be worth having.
///
/// Every op carries a surface id and a path, which on a two-row table costs
/// more than the table does. Collapsing there would turn every one-cell edit
/// into a whole-surface resend and lose the append fast path along with it. The
/// guard exists to stop a *re-sorted table* producing a hundred `set` ops, and
/// a surface that serialises to less than half a kilobyte cannot produce a
/// hundred of anything worth collapsing.
pub const COST_GUARD_FLOOR: usize = 512;

/// What one diff cost, and what it skipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffCost {
    /// Serialised bytes of the ops the differ produced, before the guard.
    pub patch_bytes: usize,
    /// Serialised bytes of the `Replace` the guard compares them against.
    pub replace_bytes: usize,
    /// Whether the guard fired and collapsed them into one `Replace`.
    pub collapsed: bool,
    /// How many surface nodes the differ descended into.
    pub nodes_walked: u64,
    /// How many child subtrees it skipped on a hash match, without descending.
    pub subtrees_skipped: u64,
}

/// Diff `next` against `prev`, appending the ops to `out`.
///
/// `out` is appended to rather than cleared: a caller diffing several surfaces
/// collects one batch.
pub fn diff(
    prev: &Surface,
    next: &Surface,
    id: SurfaceId,
    out: &mut Vec<SurfacePatch>,
) -> DiffCost {
    diff_with_hashes(prev, &hash_tree(prev), next, &hash_tree(next), id, out)
}

/// Diff with hash trees the caller already has.
///
/// The store keeps the previous surface's tree, so a re-emission hashes only
/// the new one.
pub fn diff_with_hashes(
    prev: &Surface,
    prev_hash: &HashTree,
    next: &Surface,
    next_hash: &HashTree,
    id: SurfaceId,
    out: &mut Vec<SurfacePatch>,
) -> DiffCost {
    let mut ctx = Ctx {
        id,
        ops: Vec::new(),
        cost: DiffCost::default(),
    };

    if prev_hash.hash != next_hash.hash {
        let mut path = Vec::new();
        ctx.walk(prev, prev_hash, next, next_hash, &mut path);
    }

    ctx.cost.patch_bytes = ctx.ops.iter().map(serialised_len).sum();
    if ctx.ops.is_empty() {
        return ctx.cost;
    }

    let replace = SurfacePatch::Replace {
        id,
        value: next.clone(),
    };
    ctx.cost.replace_bytes = serialised_len(&replace);

    // The guard. A single `Replace` is never worse than itself, so an op list
    // that already *is* one replace is left alone.
    let dear = ctx.cost.replace_bytes >= COST_GUARD_FLOOR
        && ctx.cost.patch_bytes as f64 > COST_GUARD * ctx.cost.replace_bytes as f64;
    if dear && !matches!(ctx.ops.as_slice(), [SurfacePatch::Replace { .. }]) {
        ctx.cost.collapsed = true;
        out.push(replace);
    } else {
        out.append(&mut ctx.ops);
    }
    ctx.cost
}

fn serialised_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0)
}

struct Ctx {
    id: SurfaceId,
    ops: Vec<SurfacePatch>,
    cost: DiffCost,
}

impl Ctx {
    fn set(&mut self, path: &[String], value: Value) {
        self.ops.push(SurfacePatch::Set {
            id: self.id,
            path: path.to_vec(),
            value,
        });
    }

    fn set_field<T: Serialize>(&mut self, path: &[String], field: &str, value: &T) {
        let mut at = path.to_vec();
        at.push(field.to_owned());
        self.set(&at, json(value));
    }

    /// Replace whatever sits at `path`: the whole surface at the root, one slot
    /// otherwise. `Replace` addresses a surface id, and a nested child has
    /// none, so below the root a replacement is a `Set`.
    fn replacement(&mut self, path: &[String], next: &Surface) {
        if path.is_empty() {
            self.ops.push(SurfacePatch::Replace {
                id: self.id,
                value: next.clone(),
            });
        } else {
            self.set(path, json(next));
        }
    }

    /// Diff a collection at row granularity: one `Set` per changed row when the
    /// lengths agree, one `Set` for the whole array when they do not.
    fn seq<T: Serialize + PartialEq>(
        &mut self,
        path: &[String],
        field: &str,
        prev: &[T],
        next: &[T],
    ) {
        if prev.len() != next.len() {
            self.set_field(path, field, &next);
            return;
        }
        for (index, (was, now)) in prev.iter().zip(next).enumerate() {
            if was != now {
                let mut at = path.to_vec();
                at.push(field.to_owned());
                at.push(index.to_string());
                self.set(&at, json(now));
            }
        }
    }

    fn walk(
        &mut self,
        prev: &Surface,
        prev_hash: &HashTree,
        next: &Surface,
        next_hash: &HashTree,
        path: &mut Vec<String>,
    ) {
        self.cost.nodes_walked += 1;

        if prev.id != next.id {
            self.set_field(path, "id", &next.id);
        }
        if prev.status != next.status {
            self.set_field(path, "status", &next.status);
        }

        if discriminant(&prev.kind) != discriminant(&next.kind) {
            self.replacement(path, next);
            return;
        }

        match (&prev.kind, &next.kind) {
            (
                SurfaceKind::Text {
                    value: was,
                    style: was_style,
                },
                SurfaceKind::Text {
                    value: now,
                    style: now_style,
                },
            ) => {
                if was_style != now_style {
                    self.kind_field(path, "style", now_style);
                }
                self.text_body(path, was, now);
            }
            (
                SurfaceKind::Markdown {
                    value: was,
                    complete: was_done,
                },
                SurfaceKind::Markdown {
                    value: now,
                    complete: now_done,
                },
            ) => {
                if was_done != now_done {
                    self.kind_field(path, "complete", now_done);
                }
                self.text_body(path, was, now);
            }
            (SurfaceKind::Stream { id: was }, SurfaceKind::Stream { id: now }) => {
                if was != now {
                    self.kind_field(path, "id", now);
                }
            }
            (
                SurfaceKind::Table {
                    columns: was_cols,
                    rows: was_rows,
                },
                SurfaceKind::Table {
                    columns: now_cols,
                    rows: now_rows,
                },
            ) => {
                if was_cols != now_cols {
                    self.kind_field(path, "columns", now_cols);
                }
                let kind = self.kind_path(path);
                self.seq(&kind, "rows", was_rows, now_rows);
            }
            (SurfaceKind::Tree { nodes: was }, SurfaceKind::Tree { nodes: now }) => {
                let kind = self.kind_path(path);
                self.seq(&kind, "nodes", was, now);
            }
            (
                SurfaceKind::Diff {
                    path: was_path,
                    hunks: was,
                },
                SurfaceKind::Diff {
                    path: now_path,
                    hunks: now,
                },
            ) => {
                if was_path != now_path {
                    self.kind_field(path, "path", now_path);
                }
                let kind = self.kind_path(path);
                self.seq(&kind, "hunks", was, now);
            }
            (
                SurfaceKind::Progress {
                    label: was_label,
                    done: was_done,
                    total: was_total,
                },
                SurfaceKind::Progress {
                    label: now_label,
                    done: now_done,
                    total: now_total,
                },
            ) => {
                if was_label != now_label {
                    self.kind_field(path, "label", now_label);
                }
                if was_done != now_done {
                    self.kind_field(path, "done", now_done);
                }
                if was_total != now_total {
                    self.kind_field(path, "total", now_total);
                }
            }
            (SurfaceKind::Task { items: was }, SurfaceKind::Task { items: now }) => {
                let kind = self.kind_path(path);
                self.seq(&kind, "items", was, now);
            }
            (
                SurfaceKind::Question {
                    prompt: was_prompt,
                    choices: was_choices,
                    multi: was_multi,
                    free: was_free,
                    default: was_default,
                    deadline_ms: was_deadline,
                },
                SurfaceKind::Question {
                    prompt: now_prompt,
                    choices: now_choices,
                    multi: now_multi,
                    free: now_free,
                    default: now_default,
                    deadline_ms: now_deadline,
                },
            ) => {
                if was_prompt != now_prompt {
                    self.kind_field(path, "prompt", now_prompt);
                }
                if was_multi != now_multi {
                    self.kind_field(path, "multi", now_multi);
                }
                if was_free != now_free {
                    self.kind_field(path, "free", now_free);
                }
                if was_default != now_default {
                    self.kind_field(path, "default", now_default);
                }
                if was_deadline != now_deadline {
                    self.kind_field(path, "deadline_ms", now_deadline);
                }
                let kind = self.kind_path(path);
                self.seq(&kind, "choices", was_choices, now_choices);
            }
            (
                SurfaceKind::Form {
                    fields: was_fields,
                    submit: was_submit,
                },
                SurfaceKind::Form {
                    fields: now_fields,
                    submit: now_submit,
                },
            ) => {
                if was_submit != now_submit {
                    self.kind_field(path, "submit", now_submit);
                }
                let kind = self.kind_path(path);
                self.seq(&kind, "fields", was_fields, now_fields);
            }
            (
                SurfaceKind::Stack {
                    dir: was_dir,
                    title: was_title,
                    collapsed: was_collapsed,
                    children: was_children,
                },
                SurfaceKind::Stack {
                    dir: now_dir,
                    title: now_title,
                    collapsed: now_collapsed,
                    children: now_children,
                },
            ) => {
                if was_dir != now_dir {
                    self.kind_field(path, "dir", now_dir);
                }
                if was_title != now_title {
                    self.kind_field(path, "title", now_title);
                }
                if was_collapsed != now_collapsed {
                    self.kind_field(path, "collapsed", now_collapsed);
                }
                self.children(path, was_children, prev_hash, now_children, next_hash);
            }
            (
                SurfaceKind::Custom {
                    kind: was_kind,
                    payload: was_payload,
                    fallback: was_fallback,
                },
                SurfaceKind::Custom {
                    kind: now_kind,
                    payload: now_payload,
                    fallback: now_fallback,
                },
            ) => {
                if was_kind != now_kind {
                    self.kind_field(path, "kind", now_kind);
                }
                if was_payload != now_payload {
                    self.kind_field(path, "payload", now_payload);
                }
                match (prev_hash.child(0), next_hash.child(0)) {
                    (Some(was_hash), Some(now_hash)) if was_hash.hash == now_hash.hash => {
                        self.cost.subtrees_skipped += 1;
                    }
                    (Some(was_hash), Some(now_hash)) => {
                        let base = path.len();
                        path.push("kind".to_owned());
                        path.push("fallback".to_owned());
                        self.walk(was_fallback, was_hash, now_fallback, now_hash, path);
                        path.truncate(base);
                    }
                    _ => {
                        if was_fallback != now_fallback {
                            self.kind_field(path, "fallback", now_fallback);
                        }
                    }
                }
            }
            // The discriminants matched above, so every pair is covered. A
            // variant added to the (non-exhaustive) enum lands here and is sent
            // whole rather than silently not sent at all.
            _ => self.replacement(path, next),
        }
    }

    /// Rule 2, for the two variants that have a body.
    fn text_body(&mut self, path: &[String], was: &str, now: &str) {
        if was == now {
            return;
        }
        // `Append` addresses a whole surface, so it is only available at the
        // root. A nested child's text change is a `Set` at its slot.
        if path.is_empty() && now.starts_with(was) {
            self.ops.push(SurfacePatch::Append {
                id: self.id,
                text: now[was.len()..].to_owned(),
            });
        } else {
            self.kind_field(path, "value", &now);
        }
    }

    /// Rule 3: match by explicit id first, then by index.
    ///
    /// A child whose hash matches its opposite number is skipped without being
    /// descended into — that is what per-node blake3 buys. A child that carries
    /// a different **id** than the slot used to hold is a *move*: the slot is
    /// written, and neither subtree is walked, so re-sorting a hundred rows
    /// costs the slots that moved rather than the whole stack.
    fn children(
        &mut self,
        path: &mut Vec<String>,
        prev: &[Surface],
        prev_hash: &HashTree,
        next: &[Surface],
        next_hash: &HashTree,
    ) {
        if prev.len() != next.len() {
            self.kind_field(path, "children", &next);
            return;
        }
        for (index, (was, now)) in prev.iter().zip(next).enumerate() {
            let (Some(was_hash), Some(now_hash)) = (prev_hash.child(index), next_hash.child(index))
            else {
                if was != now {
                    let mut at = self.kind_path(path);
                    at.push("children".to_owned());
                    at.push(index.to_string());
                    self.set(&at, json(now));
                }
                continue;
            };
            if was_hash.hash == now_hash.hash {
                self.cost.subtrees_skipped += 1;
                continue;
            }
            // Identified children that do not line up: this slot took a
            // different child, not an edit of the one that was here.
            if was.id.is_some() && now.id.is_some() && was.id != now.id {
                let mut at = self.kind_path(path);
                at.push("children".to_owned());
                at.push(index.to_string());
                self.set(&at, json(now));
                continue;
            }
            let base = path.len();
            path.push("kind".to_owned());
            path.push("children".to_owned());
            path.push(index.to_string());
            self.walk(was, was_hash, now, now_hash, path);
            path.truncate(base);
        }
    }

    fn kind_path(&self, path: &[String]) -> Vec<String> {
        let mut at = path.to_vec();
        at.push("kind".to_owned());
        at
    }

    fn kind_field<T: Serialize>(&mut self, path: &[String], field: &str, value: &T) {
        let kind = self.kind_path(path);
        self.set_field(&kind, field, value);
    }
}

fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The variant's wire tag, which is what "discriminant changed" means.
fn discriminant(kind: &SurfaceKind) -> &'static str {
    match kind {
        SurfaceKind::Text { .. } => "text",
        SurfaceKind::Table { .. } => "table",
        SurfaceKind::Tree { .. } => "tree",
        SurfaceKind::Diff { .. } => "diff",
        SurfaceKind::Progress { .. } => "progress",
        SurfaceKind::Stream { .. } => "stream",
        SurfaceKind::Task { .. } => "task",
        SurfaceKind::Question { .. } => "question",
        SurfaceKind::Form { .. } => "form",
        SurfaceKind::Stack { .. } => "stack",
        SurfaceKind::Markdown { .. } => "markdown",
        SurfaceKind::Custom { .. } => "custom",
        // A variant this build does not know. Treated as its own
        // discriminant, so it is never mistaken for a neighbour.
        _ => "?",
    }
}

/// Apply one patch to a surface.
///
/// The inverse of [`diff`], and the reason the proptest can assert that a patch
/// sequence reconstructs its target. Clients have their own copy of this in
/// their own language; this one is what the property is checked against.
///
/// # Errors
///
/// [`ApplyError`] when the patch does not fit the surface it was given.
pub fn apply(surface: &mut Surface, patch: &SurfacePatch) -> Result<(), ApplyError> {
    match patch {
        SurfacePatch::Replace { value, .. } => {
            *surface = value.clone();
            Ok(())
        }
        SurfacePatch::Append { text, .. } => match &mut surface.kind {
            SurfaceKind::Text { value, .. } | SurfaceKind::Markdown { value, .. } => {
                value.push_str(text);
                Ok(())
            }
            other => Err(ApplyError::NotAppendable {
                kind: discriminant(other).to_owned(),
            }),
        },
        SurfacePatch::Set { path, value, .. } => {
            let mut json =
                serde_json::to_value(&*surface).map_err(|e| ApplyError::NotASurface {
                    message: e.to_string(),
                })?;
            set_at(&mut json, path, value.clone())
                .ok_or_else(|| ApplyError::NoSuchField { path: path.clone() })?;
            *surface = serde_json::from_value(json).map_err(|e| ApplyError::NotASurface {
                message: e.to_string(),
            })?;
            Ok(())
        }
        SurfacePatch::Remove { .. } => Err(ApplyError::Removed),
        // An op this build does not know is refused rather than ignored: a
        // client that silently drops a patch is a client that quietly diverges.
        other => Err(ApplyError::NotASurface {
            message: format!("unknown patch op: {other:?}"),
        }),
    }
}

/// Walk `path` and write `value`. `None` when the path does not lead anywhere.
///
/// The last segment may name a key that is not there — `progress.total` is
/// skipped when it is `None`, and setting it has to put it back.
fn set_at(root: &mut Value, path: &[String], value: Value) -> Option<()> {
    let (last, parents) = path.split_last()?;
    let mut at = root;
    for segment in parents {
        at = descend(at, segment)?;
    }
    match at {
        Value::Object(map) => {
            map.insert(last.clone(), value);
            Some(())
        }
        Value::Array(items) => {
            let index: usize = last.parse().ok()?;
            let slot = items.get_mut(index)?;
            *slot = value;
            Some(())
        }
        _ => None,
    }
}

fn descend<'a>(at: &'a mut Value, segment: &str) -> Option<&'a mut Value> {
    match at {
        Value::Object(map) => map.get_mut(segment),
        Value::Array(items) => items.get_mut(segment.parse::<usize>().ok()?),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use orrery_proto::{StackDir, Surface, SurfaceId, SurfaceKind, SurfacePatch};

    use super::{apply, diff};
    use crate::ApplyError;

    fn text(value: &str) -> Surface {
        Surface::new(SurfaceKind::Text {
            value: value.to_owned(),
            style: None,
        })
    }

    #[test]
    fn a_set_puts_back_a_field_that_was_skipped() {
        let prev = Surface::new(SurfaceKind::Progress {
            label: "linking".into(),
            done: None,
            total: None,
        });
        let next = Surface::new(SurfaceKind::Progress {
            label: "linking".into(),
            done: Some(3),
            total: Some(9),
        });
        let mut out = Vec::new();
        diff(&prev, &next, SurfaceId::new(), &mut out);
        let mut rebuilt = prev;
        for op in &out {
            apply(&mut rebuilt, op).expect("applies");
        }
        assert_eq!(rebuilt, next);
    }

    #[test]
    fn append_to_a_stream_is_refused_here() {
        let mut stream = Surface::new(SurfaceKind::Stream {
            id: "stdout".into(),
        });
        let err = apply(
            &mut stream,
            &SurfacePatch::Append {
                id: SurfaceId::new(),
                text: "hi".into(),
            },
        )
        .expect_err("a stream carries no body in the surface type");
        assert!(matches!(err, ApplyError::NotAppendable { .. }));
    }

    #[test]
    fn a_child_count_change_rewrites_the_children() {
        let prev = Surface::new(SurfaceKind::Stack {
            dir: StackDir::Column,
            title: None,
            collapsed: false,
            children: vec![text("a")],
        });
        let next = Surface::new(SurfaceKind::Stack {
            dir: StackDir::Column,
            title: None,
            collapsed: false,
            children: vec![text("a"), text("b")],
        });
        let mut out = Vec::new();
        diff(&prev, &next, SurfaceId::new(), &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        let mut rebuilt = prev;
        apply(&mut rebuilt, &out[0]).expect("applies");
        assert_eq!(rebuilt, next);
    }
}
