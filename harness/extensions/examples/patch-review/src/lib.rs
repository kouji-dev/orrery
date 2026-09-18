//! A ported example extension: a patch review, as a diff and a question.
//!
//! # What was ported
//!
//! Every review loop in every coding agent ends the same way: print a patch,
//! print `[y/N]`, read a line. That is three pieces of drawing code — the
//! colouring of `+`/`-`, the prompt, and the echo — and it works in a terminal
//! and nowhere else.
//!
//! Here the patch is a [`Diff`](orrery_proto::SurfaceKind::Diff) and the prompt
//! is a [`Question`](orrery_proto::SurfaceKind::Question). ratatui draws hunks
//! and a key-driven chooser, Ink draws a component, `--json` emits both as
//! data, and the answer comes back as an intent — so **the asking step pauses,
//! not the kernel**. The default is what an unattended run does, which is why
//! it is `skip`: nothing applies a patch because nobody was watching.
//!
//! # It draws nothing
//!
//! No `std::io`, no `print!`, no terminal crate, no width, no colour.
//! `ported::draws_nothing` asserts that over this file.
//!
//! Implementation plan: `harness/docs/plans/09-surfaces.md` (phase 4, §8)

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, SurfaceBuilders, ToolDef};
use orrery_proto::{Choice, DiffLine, DiffLineKind, Hunk, Outcome, StackDir, SurfaceId};
use serde_json::Value;
use uuid::Uuid;

/// This extension's `orrery.toml`, compiled in.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The id a review re-emits under, so a second look patches the first.
pub const REVIEW: SurfaceId =
    SurfaceId::from_uuid(Uuid::from_u128(0x0193_7ce5_0000_7000_8000_0000_0000_0002));

/// The review, as an extension.
#[derive(Clone, Copy, Debug, Default)]
pub struct PatchReview;

fn hunks(input: &Value) -> Vec<Hunk> {
    input
        .get("hunks")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|h| Hunk {
                    old_start: number(h, "old_start"),
                    old_lines: number(h, "old_lines"),
                    new_start: number(h, "new_start"),
                    new_lines: number(h, "new_lines"),
                    lines: h
                        .get("lines")
                        .and_then(Value::as_array)
                        .map(|lines| lines.iter().map(line).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn number(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or(0)
}

fn line(value: &Value) -> DiffLine {
    DiffLine {
        kind: match value.get("op").and_then(Value::as_str) {
            Some("add") => DiffLineKind::Add,
            Some("del" | "remove") => DiffLineKind::Remove,
            _ => DiffLineKind::Context,
        },
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

/// `a/b/c.rs` shown as `c.rs`: the prompt has to fit on one line in a client
/// that is 40 columns wide, and the whole path is in the diff above it.
fn leaf(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[async_trait]
impl NativeExtension for PatchReview {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/examples/patch-review/orrery.toml"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new("review")
                .described(
                    "Show a patch and ask whether to apply it. The answer comes back \
                     as an intent; unattended, nothing is applied.",
                )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "The file the patch touches." },
                        "hunks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_start": { "type": "integer" },
                                    "old_lines": { "type": "integer" },
                                    "new_start": { "type": "integer" },
                                    "new_lines": { "type": "integer" },
                                    "lines": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "op": { "enum": ["context", "add", "del"] },
                                                "text": { "type": "string" }
                                            },
                                            "required": ["op", "text"]
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "required": ["path", "hunks"]
                })),
        ]
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        if tool != "review" {
            return Ok(Outcome::Failed {
                code: "no-such-tool".to_owned(),
                message: format!("`{tool}` is not a tool this extension contributes"),
            });
        }

        let path = input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let hunks = hunks(&input);
        if hunks.is_empty() {
            return Ok(Outcome::Failed {
                code: "empty-patch".to_owned(),
                message: format!(
                    "the patch for `{path}` has no hunks: there is nothing to show and \
                     nothing to answer about"
                ),
            });
        }

        let changed = hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind != DiffLineKind::Context)
            .count();

        let diff = ctx.ui.diff(path.clone(), hunks);
        let question = ctx.ui.question_full(
            format!("Apply {changed} changed lines to {}?", leaf(&path)),
            vec![
                Choice {
                    value: "apply".to_owned(),
                    label: "Apply".to_owned(),
                },
                Choice {
                    value: "skip".to_owned(),
                    label: "Skip".to_owned(),
                },
                Choice {
                    value: "explain".to_owned(),
                    label: "Explain the change".to_owned(),
                },
            ],
            false,
            false,
            // Unattended, `default` resolves the question. Nothing applies a
            // patch because nobody was there to say yes.
            Some("skip".to_owned()),
            None,
        );

        let review = ctx.ui.with_id(
            ctx.ui.stack(StackDir::Column, vec![diff, question]),
            REVIEW,
        );

        Ok(Outcome::Ok {
            surface: Some(review),
            value: Some(serde_json::json!({ "path": path, "changed_lines": changed })),
        })
    }
}
