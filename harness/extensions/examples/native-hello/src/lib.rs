//! The `native` worked example: one tool, compiled into the harness.
//!
//! This is the fourth writing of `hello.parity`. The other three are
//! `../node-hello` (a child process speaking JSON-RPC), `../wasm-hello-rs` and
//! `../wasm-hello-go` (components built from the same `.wit`). All four return
//! the same [`Surface`](orrery_proto::surface::Surface), and
//! `orrery-harness/tests/parity.rs` asserts it.
//!
//! # `native` is a runtime, not a back door
//!
//! Nothing here is privileged. The manifest is the same TOML a third party
//! ships, read by the same parser; the call arrives through the same
//! `ExtensionTable`, is policy-checked by the same engine, and is bounded by
//! the same [`ToolBudget`](orrery_ext_api::ToolBudget). The only difference
//! between this crate and `node-hello` is where the code was compiled
//! (translation #14).
//!
//! What `native` buys is the absence of a process boundary: no spawn, no
//! framing, no serialisation per call. What it costs is the sandbox — a
//! compiled-in extension shares the address space, so it is
//! capability-withholding rather than contained. `wasm` is the runtime with a
//! real boundary. See `extensions/README.md` for the threat model in full.
//!
//! Implementation plan: `harness/docs/plans/18-writing-an-extension.md` (Task 4)

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, ToolDef};
use orrery_proto::Outcome;
use serde_json::Value;

/// The manifest, `include_str!`-ed so the shipped file and the compiled crate
/// cannot drift apart.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The example extension.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeHello;

impl NativeHello {
    /// The bundle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NativeExtension for NativeHello {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/examples/native-hello/orrery.toml"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new("parity")
                .described(
                    "Describe a fixed two-row table. The same tool exists in the node and \
                     wasm examples and returns exactly this, which is how the harness proves \
                     the kernel cannot tell the runtimes apart.",
                )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            // Nothing is required: the point of the comparison is dispatch, not
            // policy, so every runtime can be granted nothing at all and still
            // answer.
        ]
    }

    async fn call(&self, tool: &str, _input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        match tool {
            "parity" => Ok(Outcome::Ok {
                surface: Some(ctx.ui.table(
                    ["key", "value"],
                    [["tool", "parity"], ["runtime", "irrelevant"]],
                )),
                value: None,
            }),
            other => Err(HostError::NoSuchTool {
                ext: ctx.ext.clone(),
                tool: other.to_owned(),
            }),
        }
    }
}
