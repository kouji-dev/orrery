//! The built-in tool bundle: read, write, edit, bash, grep and glob, shipped as a first-party extension like any other.
//!
//! # There is no privileged path
//!
//! `builtin.read` is not a function the kernel calls. It is an extension with a
//! manifest, loaded by the `native` runtime, reached through
//! `Registry::dispatch`, checked by the policy engine and bounded by a
//! [`ToolBudget`](orrery_ext_api::ToolBudget) like anything a third party ships
//! (translation #14). The only difference is where the code was compiled.
//!
//! # Every ceiling is applied while the work happens
//!
//! §4.5's objective-5 complaint is that a limit checked *after* the fact has
//! already let the bytes into memory. So nothing here calls `read_to_end`:
//! [`read`](BuiltinTools) asks the broker for at most `output_bytes` and reports
//! [`Outcome::Truncated`] when there was more, `grep` does the same per file,
//! and `bash`'s output is pumped by the broker with a closed pipe at the
//! ceiling.
//!
//! # What the broker does not offer, and what that costs
//!
//! [`BrokerFacade`](orrery_ext_api::BrokerFacade) has five methods — read,
//! write, spawn, fetch, credential — and **no directory listing**. `grep` and
//! `glob` therefore discover *names* with `std::fs::read_dir`, and read every
//! *byte* through the broker under the call's ceiling. That is a real gap, and
//! it is written down rather than hidden: a `list` method on the facade (a
//! non-breaking addition under `orrery-ext/1`, see plan 06's open question 3)
//! would close it, and until it exists a `read` deny does not stop a path from
//! being named in a glob result.
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md` (Task 4)

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod bash;
mod files;
mod search;

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, ToolDef};
use orrery_proto::{Aspect, Outcome};
use serde_json::Value;

/// The bundle's manifest, parsed by the same parser a third party's goes
/// through.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The six tools.
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinTools;

impl BuiltinTools {
    /// The bundle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// A tool call whose arguments did not make sense.
///
/// A **value**, not an error: the model wrote them and the model can fix them,
/// which is only possible if it is told what was wrong.
pub(crate) fn bad_input(message: impl std::fmt::Display) -> Outcome {
    Outcome::Failed {
        code: "invalid-input".to_owned(),
        message: message.to_string(),
    }
}

/// A successful outcome that both shows text and feeds it back.
pub(crate) fn text_outcome(ctx: &CallCtx, text: String) -> Outcome {
    let surface = ctx.ui.markdown(text.clone(), true);
    Outcome::Ok {
        surface: Some(surface),
        value: Some(serde_json::json!({ "text": text })),
    }
}

/// The string field `name`, or a `Failed` outcome saying which one is missing.
///
/// The `Err` half is an [`Outcome`], which clippy notes is a large variant to
/// carry in a `Result`. It stays: the alternative is a second error type that
/// every tool would then have to convert into an outcome, which is the
/// duplication `Outcome` exists to prevent, and this is one allocation per
/// malformed call rather than per call.
#[allow(clippy::result_large_err)]
pub(crate) fn string_arg(input: &Value, name: &str) -> Result<String, Outcome> {
    input
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| bad_input(format!("`{name}` is required and must be a string")))
}

#[async_trait]
impl NativeExtension for BuiltinTools {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "harness/extensions/crates/orrery-ext-tools-builtin/orrery.toml"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new("read")
                .described(
                    "Read a file. Returns at most the call's output ceiling; \
                     a longer file comes back truncated and says so.",
                )
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "The file to read." },
                        "limit": {
                            "type": "integer",
                            "description": "How many bytes at most. Capped by the call's ceiling."
                        }
                    },
                    "required": ["path"]
                }))
                .requiring([Aspect::Read]),
            ToolDef::new("write")
                .described("Replace a file's contents. All-or-nothing.")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }))
                .atomic(true)
                .requiring([Aspect::Write]),
            ToolDef::new("edit")
                .described("Replace one run of text in a file with another. All-or-nothing.")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" },
                        "replace_all": { "type": "boolean" }
                    },
                    "required": ["path", "old_text", "new_text"]
                }))
                .atomic(true)
                .requiring([Aspect::Read, Aspect::Write]),
            ToolDef::new("bash")
                .described("Run a shell command, contained and under the call's wall clock.")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "cwd": { "type": "string" },
                        "timeout_ms": { "type": "integer" }
                    },
                    "required": ["command"]
                }))
                .requiring([Aspect::Spawn]),
            ToolDef::new("grep")
                .described("Search a tree for a pattern, line by line, bounded per file.")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path": { "type": "string" },
                        "max_matches": { "type": "integer" }
                    },
                    "required": ["pattern"]
                }))
                .requiring([Aspect::Read]),
            ToolDef::new("glob")
                .described("List the files under a directory matching a glob.")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path": { "type": "string" }
                    },
                    "required": ["pattern"]
                }))
                .requiring([Aspect::Read]),
        ]
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        let outcome = match tool {
            "read" => files::read(input, ctx).await,
            "write" => files::write(input, ctx).await,
            "edit" => files::edit(input, ctx).await,
            "bash" => bash::run(input, ctx).await,
            "grep" => search::grep(input, ctx).await,
            "glob" => search::glob(input, ctx).await,
            other => {
                return Err(HostError::NoSuchTool {
                    ext: ctx.ext.clone(),
                    tool: other.to_owned(),
                });
            }
        };
        // Both halves are an [`Outcome`]: the `Err` side is how a tool stops
        // early with a refusal or a bad-input report, not a failure of the
        // harness to carry the call.
        Ok(match outcome {
            Ok(o) | Err(o) => o,
        })
    }
}
