//! A ported example extension: the workspace census, as surfaces.
//!
//! # What was ported
//!
//! `xtask deps-check` prints a report: a line per crate, a line per violation,
//! and a colour if the terminal takes one. That is drawing code, and it works
//! in exactly one client. This extension is the same report described through
//! [`ctx.ui`](orrery_ext_api::SurfaceSink) — a section holding a markdown
//! summary, a table and a footer — and it therefore works in every client
//! there is or will be.
//!
//! # It draws nothing
//!
//! There is no `std::io` here, no `print!`, no terminal crate, and no width. It
//! never learns whether it is being read in ratatui, in Ink, in the ADE or in
//! `--json`. `ported::draws_nothing` in `harness/clients/ported` asserts that
//! mechanically, over this file.
//!
//! Implementation plan: `harness/docs/plans/09-surfaces.md` (phase 4, §8)

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, SurfaceBuilders, ToolDef};
use orrery_proto::{Outcome, SurfaceId, TextStyle};
use serde_json::Value;
use uuid::Uuid;

/// This extension's `orrery.toml`, compiled in.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The id this extension re-emits its census under.
///
/// Extension-minted and stable, which is what makes re-emission the whole API:
/// the kernel keys the store by `(turn, id)` and diffs what changed. A fresh
/// id per call would make every census a new surface.
pub const CENSUS: SurfaceId =
    SurfaceId::from_uuid(Uuid::from_u128(0x0193_7ce5_0000_7000_8000_0000_0000_0001));

/// The census, as an extension.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceCensus;

/// One crate, as the caller describes it.
struct Member {
    name: String,
    area: String,
    published: bool,
}

fn members(input: &Value) -> Vec<Member> {
    input
        .get("crates")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|c| Member {
                    name: c
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_owned(),
                    area: c
                        .get("area")
                        .and_then(Value::as_str)
                        .unwrap_or("workspace")
                        .to_owned(),
                    published: c.get("published").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl NativeExtension for WorkspaceCensus {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/examples/workspace-census/orrery.toml"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new("census")
                .described(
                    "Show the workspace census: one row per crate, with the area it \
                     lives in and whether it publishes.",
                )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "crates": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "area": { "type": "string" },
                                    "published": { "type": "boolean" }
                                },
                                "required": ["name", "area"]
                            }
                        }
                    },
                    "required": ["crates"]
                })),
        ]
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        if tool != "census" {
            return Ok(Outcome::Failed {
                code: "no-such-tool".to_owned(),
                message: format!("`{tool}` is not a tool this extension contributes"),
            });
        }

        let members = members(&input);
        let published = members.iter().filter(|m| m.published).count();

        let summary = ctx.ui.markdown(
            format!(
                "**{}** crates, **{published}** of them published.",
                members.len()
            ),
            true,
        );
        let table = ctx.ui.table(
            ["crate", "area", "publish"],
            members.iter().map(|m| {
                [
                    m.name.clone(),
                    m.area.clone(),
                    if m.published { "published" } else { "private" }.to_owned(),
                ]
            }),
        );
        let footer = ctx.ui.styled(
            "publish = false is a crate a community extension cannot depend on.",
            TextStyle::Muted,
        );
        let census = ctx.ui.with_id(
            ctx.ui
                .section("workspace census", false, vec![summary, table, footer]),
            CENSUS,
        );

        Ok(Outcome::Ok {
            surface: Some(census),
            // What the model reads is not what the person sees: the counts, not
            // the table.
            value: Some(serde_json::json!({
                "crates": members.len(),
                "published": published,
            })),
        })
    }
}
