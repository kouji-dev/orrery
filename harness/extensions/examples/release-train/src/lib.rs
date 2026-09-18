//! A ported example extension: a release train, as tasks, progress and one
//! custom surface.
//!
//! # What was ported
//!
//! A release script prints a checklist, redraws a bar with `\r`, and — if
//! somebody once loved it — draws a little ASCII timeline. All three are
//! drawing code. Here they are a [`Task`](orrery_proto::SurfaceKind::Task)
//! list, a [`Progress`](orrery_proto::SurfaceKind::Progress) surface and a
//! [`Custom`](orrery_proto::SurfaceKind::Custom) one.
//!
//! # The custom surface is the interesting half
//!
//! A timeline is the case the core vocabulary does not have: stages on an axis,
//! with the width meaning elapsed time. So this extension ships its own
//! `example-release-train.timeline` — and with it **a fallback that has to be
//! worth reading**, because every client without a renderer for that kind draws
//! the fallback instead, and `--json` prints it beside the payload so a lazy
//! one shows up in CI (§6.2). The rule this extension keeps: *the fallback
//! names everything the payload names*. `ported::the_fallback_is_informative`
//! asserts exactly that, stage by stage.
//!
//! # It draws nothing
//!
//! No `std::io`, no `print!`, no `\r`, no terminal crate, no width.
//!
//! Implementation plan: `harness/docs/plans/09-surfaces.md` (phase 4, §8)

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, SurfaceBuilders, ToolDef};
use orrery_proto::{Outcome, StackDir, Status, Surface, SurfaceId, TaskItem};
use serde_json::Value;
use uuid::Uuid;

/// This extension's `orrery.toml`, compiled in.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The id the train re-emits under: the second call patches the first.
pub const TRAIN: SurfaceId =
    SurfaceId::from_uuid(Uuid::from_u128(0x0193_7ce5_0000_7000_8000_0000_0000_0003));

/// The custom surface kind, namespaced by the extension that owns it.
pub const TIMELINE: &str = "example-release-train.timeline";

/// The release train, as an extension.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReleaseTrain;

/// One stage of the train, as the caller describes it.
struct Stage {
    id: String,
    label: String,
    status: Status,
}

fn status_of(name: Option<&str>) -> Status {
    match name {
        Some("done") => Status::Done,
        Some("running") => Status::Running,
        Some("failed") => Status::Failed,
        Some("cancelled") => Status::Cancelled,
        _ => Status::Pending,
    }
}

fn stages(input: &Value) -> Vec<Stage> {
    input
        .get("stages")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|s| Stage {
                    id: s.get("id").and_then(Value::as_str).unwrap_or("?").to_owned(),
                    label: s
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_owned(),
                    status: status_of(s.get("status").and_then(Value::as_str)),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn mark(status: Status) -> &'static str {
    match status {
        Status::Done => "shipped",
        Status::Running => "in flight",
        Status::Failed => "derailed",
        Status::Cancelled => "called off",
        // `Status` is `#[non_exhaustive]`: a variant we have never heard of is
        // still a state worth naming, and a fallback that dropped it would be
        // exactly the lazy fallback 6.2 is about.
        _ => "waiting",
    }
}

#[async_trait]
impl NativeExtension for ReleaseTrain {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/examples/release-train/orrery.toml"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new("status")
                .described(
                    "Show where a release is: a checklist of stages, how far along it \
                     is, and a timeline.",
                )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "release": { "type": "string" },
                        "stages": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "label": { "type": "string" },
                                    "status": {
                                        "enum": ["pending", "running", "done", "failed", "cancelled"]
                                    }
                                },
                                "required": ["id", "label", "status"]
                            }
                        }
                    },
                    "required": ["release", "stages"]
                })),
        ]
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        if tool != "status" {
            return Ok(Outcome::Failed {
                code: "no-such-tool".to_owned(),
                message: format!("`{tool}` is not a tool this extension contributes"),
            });
        }

        let release = input
            .get("release")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_owned();
        let stages = stages(&input);
        let total = stages.len() as u64;
        let done = stages.iter().filter(|s| s.status == Status::Done).count() as u64;

        let tasks = ctx.ui.task(
            stages
                .iter()
                .map(|s| TaskItem {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    status: s.status,
                })
                .collect(),
        );
        let progress = ctx
            .ui
            .progress(release.clone(), Some(done), Some(total));

        // The custom surface, and the fallback that has to stand in for it.
        let payload = serde_json::json!({
            "release": release,
            "stages": stages
                .iter()
                .map(|s| serde_json::json!({
                    "id": s.id,
                    "label": s.label,
                    "state": mark(s.status),
                }))
                .collect::<Vec<_>>(),
        });
        let fallback = self.fallback(ctx, &release, done, total, &stages);
        let timeline = ctx.ui.custom(TIMELINE, payload, fallback);

        let train = ctx.ui.with_id(
            ctx.ui
                .stack(StackDir::Column, vec![tasks, progress, timeline]),
            TRAIN,
        );

        Ok(Outcome::Ok {
            surface: Some(train),
            value: Some(serde_json::json!({
                "release": release,
                "done": done,
                "total": total,
            })),
        })
    }
}

impl ReleaseTrain {
    /// What a client with no timeline renderer draws instead.
    ///
    /// Not `text("open the web UI")`. It names the release, says how far the
    /// train has got, and then names **every stage the payload names**, with
    /// its state — which is everything the rich version would have shown,
    /// minus the axis.
    fn fallback(
        &self,
        ctx: &CallCtx,
        release: &str,
        done: u64,
        total: u64,
        stages: &[Stage],
    ) -> Surface {
        let headline = ctx.ui.text(format!(
            "{release}: {done} of {total} stages shipped, in order —"
        ));
        let table = ctx.ui.table(
            ["stage", "state"],
            stages
                .iter()
                .map(|s| [s.label.clone(), mark(s.status).to_owned()]),
        );
        ctx.ui
            .stack(StackDir::Column, vec![headline, table])
    }
}
