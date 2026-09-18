//! Git tools over gitoxide, reusing the knowledge in ade/src-tauri/src/git/.
//!
//! Five read-only verbs — `status`, `log`, `show`, `diff`, `blame` — as a
//! first-party extension loaded through `orrery-host` like any third-party one:
//! the ledger shows it, a deny rule disables it, `orrery ext test` runs it.
//!
//! # Why this is read-only, and what happens when it is not
//!
//! The scaffold's manifest declared `branch` and `commit` and asked for
//! `write = ["$WORKSPACE/.git/**"]` and `spawn = ["git"]`. None of those three
//! is implemented, so none of them is declared any more: a manifest that lists
//! a tool the extension does not have makes the ledger a lie, and a grant it
//! does not use is a capability handed over for nothing.
//!
//! When the write verbs land they go through the broker, under
//! `write = ["$WORKSPACE/.git/**"]`, and the worktree stays unwritable here —
//! that is the difference between "this can commit" and "this can edit", and
//! the second is `builtin.write`'s job under its own grant.
//!
//! # How a policy check happens at all
//!
//! gitoxide opens the object database itself, with `std::fs`, which the broker
//! cannot mediate. So before any verb touches a repository, this bundle asks
//! the broker to read the repository marker — a real, policy-checked
//! [`Aspect::Read`](orrery_proto::Aspect) call against the path in question. A
//! **denial** stops the verb and becomes [`Outcome::Denied`]; anything else
//! (the marker is a directory, or does not exist) does not, because the probe
//! is asking "may I", not "is it there".
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod repo;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orrery_ext_api::{BrokerError, CallCtx, HostError, NativeExtension, ReadRequest, ToolDef};
use orrery_proto::{Aspect, Outcome};
use serde_json::{Value, json};

use crate::repo::GitError;

/// The manifest this extension ships, parsed by the same parser a third
/// party's is.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// How many commits `log` returns when the model does not say.
const DEFAULT_LOG_LIMIT: usize = 20;
/// The most it will return however large a number the model asks for. A log
/// with no ceiling is a context window spent on history.
const MAX_LOG_LIMIT: usize = 500;

/// The git bundle.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitTools;

impl GitTools {
    /// The bundle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// The repository a verb is about: the `repo` argument, or the workspace.
fn repo_arg(input: &Value) -> PathBuf {
    input
        .get("repo")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// Ask the broker whether this call may read inside `dir`.
///
/// Returns the refusal, when there is one. A missing file is not a refusal:
/// gitoxide will say so in its own words a moment later, and inventing a
/// second vocabulary for "there is no repository there" helps nobody.
async fn may_read(ctx: &CallCtx, dir: &Path) -> Option<Outcome> {
    // One byte. The probe is a permission question, not a read.
    match ctx.broker.read(ReadRequest::new(dir.join(".git"), 1)).await {
        Err(e @ BrokerError::Denied { .. }) | Err(e @ BrokerError::Cancelled { .. }) => {
            Some(e.into_outcome())
        }
        _ => None,
    }
}

/// What the model is told when gitoxide refused.
fn failed(e: &GitError) -> Outcome {
    let code = match e {
        GitError::NotARepository { .. } => "not_a_repository",
        GitError::NoWorktree { .. } => "no_worktree",
        GitError::NoSuchRevision(_) => "no_such_revision",
        GitError::Failed { .. } => "git",
    };
    Outcome::Failed {
        code: code.to_owned(),
        message: e.to_string(),
    }
}

#[async_trait]
impl NativeExtension for GitTools {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn tools(&self) -> Vec<ToolDef> {
        let repo_field = json!({
            "repo": {
                "type": "string",
                "description": "Path to the repository. Defaults to the workspace root.",
            },
        });
        let with = |extra: Value| -> Value {
            let mut props = repo_field.clone();
            if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
                for (k, v) in e {
                    p.insert(k.clone(), v.clone());
                }
            }
            json!({ "type": "object", "properties": props })
        };
        // Every one of these `requires` `read`: without the list, a missing
        // grant is either a whole failed extension or a surprise at the first
        // call. With it, the ledger says at load time which verbs cannot work.
        let reads = [Aspect::Read];

        vec![
            ToolDef::new("status")
                .described(
                    "What is different between HEAD, the index and the worktree. \
                     One row per path with a state of M, A, D, R or ?.",
                )
                .with_schema(with(json!({})))
                .requiring(reads),
            ToolDef::new("log")
                .described(
                    "Commits behind a revision, newest first: short id, subject, \
                     author and unix time.",
                )
                .with_schema(with(json!({
                    "rev": { "type": "string", "description": "Where to start. Defaults to HEAD." },
                    "limit": {
                        "type": "integer",
                        "description":
                            "How many commits, at most 500. Defaults to 20.",
                        "minimum": 1,
                    },
                })))
                .requiring(reads),
            ToolDef::new("show")
                .described("One commit: who wrote it, what it said, and which paths it touched.")
                .with_schema(with(json!({
                    "rev": { "type": "string", "description": "Which commit. Defaults to HEAD." },
                })))
                .requiring(reads),
            ToolDef::new("diff")
                .described(
                    "Which paths differ between two revisions. With only `to`, \
                     compares it against its own parent.",
                )
                .with_schema(with(json!({
                    "from": { "type": "string", "description": "The older side." },
                    "to": { "type": "string", "description": "The newer side. Defaults to HEAD." },
                })))
                .requiring(reads),
            ToolDef::new("blame")
                .described("Who last touched each line of a file, as of a revision.")
                .with_schema(with(json!({
                    "path": {
                        "type": "string",
                        "description": "The file, relative to the repository root.",
                    },
                    "rev": { "type": "string", "description": "As of when. Defaults to HEAD." },
                })))
                .requiring(reads),
        ]
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        let dir = repo_arg(&input);
        if let Some(denied) = may_read(ctx, &dir).await {
            return Ok(denied);
        }
        // Cancellation between the check and the work is not a hypothetical:
        // `status` on a large tree is the slowest thing in this crate.
        if ctx.is_cancelled() {
            return Ok(Outcome::Cancelled {
                reason: orrery_proto::CancelReason::User,
            });
        }

        let repository = match repo::open(&dir) {
            Ok(r) => r,
            Err(e) => return Ok(failed(&e)),
        };
        let rev = input.get("rev").and_then(Value::as_str);

        Ok(match tool {
            "status" => match repo::status(&repository) {
                Ok(changes) => changes_outcome(ctx, "status", &changes),
                Err(e) => failed(&e),
            },
            "log" => {
                let limit = input
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map_or(DEFAULT_LOG_LIMIT, |n| {
                        usize::try_from(n).unwrap_or(DEFAULT_LOG_LIMIT)
                    })
                    .clamp(1, MAX_LOG_LIMIT);
                match repo::log(&repository, rev, limit) {
                    Ok(entries) => entries_outcome(ctx, &entries),
                    Err(e) => failed(&e),
                }
            }
            "show" => match repo::show(&repository, rev) {
                Ok((entry, changes)) => {
                    let surface = ctx.ui.table(
                        ["path", "state", "from"],
                        changes.iter().map(|c| {
                            vec![
                                c.path.clone(),
                                c.state.clone(),
                                c.old_path.clone().unwrap_or_default(),
                            ]
                        }),
                    );
                    Outcome::Ok {
                        surface: Some(surface),
                        value: Some(json!({
                            "commit": entry_json(&entry),
                            "changes": changes.iter().map(change_json).collect::<Vec<_>>(),
                        })),
                    }
                }
                Err(e) => failed(&e),
            },
            "diff" => {
                let from = input.get("from").and_then(Value::as_str);
                let to = input.get("to").and_then(Value::as_str);
                match repo::diff(&repository, from, to) {
                    Ok(changes) => changes_outcome(ctx, "diff", &changes),
                    Err(e) => failed(&e),
                }
            }
            "blame" => {
                let Some(path) = input.get("path").and_then(Value::as_str) else {
                    return Ok(Outcome::Failed {
                        code: "bad_request".to_owned(),
                        message: "`blame` needs a `path`".to_owned(),
                    });
                };
                match repo::blame(&repository, path, rev) {
                    Ok(lines) => {
                        let surface = ctx.ui.table(
                            ["line", "commit", "author"],
                            lines
                                .iter()
                                .map(|l| vec![l.line.to_string(), l.sha.clone(), l.author.clone()]),
                        );
                        Outcome::Ok {
                            surface: Some(surface),
                            value: Some(json!({
                                "path": path,
                                "lines": lines
                                    .iter()
                                    .map(|l| json!({
                                        "line": l.line,
                                        "commit": l.sha,
                                        "author": l.author,
                                    }))
                                    .collect::<Vec<_>>(),
                            })),
                        }
                    }
                    Err(e) => failed(&e),
                }
            }
            other => {
                return Err(HostError::NoSuchTool {
                    ext: ctx.ext.clone(),
                    tool: other.to_owned(),
                });
            }
        })
    }
}

fn change_json(c: &repo::Change) -> Value {
    let mut v = json!({ "path": c.path, "state": c.state });
    if let (Some(old), Some(map)) = (&c.old_path, v.as_object_mut()) {
        map.insert("old_path".to_owned(), json!(old));
    }
    v
}

fn entry_json(e: &repo::Entry) -> Value {
    json!({ "sha": e.sha, "subject": e.subject, "author": e.author, "time": e.time })
}

/// A table for a person and the same rows as JSON for the model.
///
/// Both, not one: the surface is what a client draws, and `value` is what goes
/// back into the context window. Feeding a rendered table to a model is how a
/// tool result becomes unparseable the first time a path contains a space.
fn changes_outcome(ctx: &CallCtx, what: &str, changes: &[repo::Change]) -> Outcome {
    let surface = if changes.is_empty() {
        ctx.ui.text(format!("{what}: nothing to report"))
    } else {
        ctx.ui.table(
            ["path", "state", "from"],
            changes.iter().map(|c| {
                vec![
                    c.path.clone(),
                    c.state.clone(),
                    c.old_path.clone().unwrap_or_default(),
                ]
            }),
        )
    };
    Outcome::Ok {
        surface: Some(surface),
        value: Some(json!(changes.iter().map(change_json).collect::<Vec<_>>())),
    }
}

fn entries_outcome(ctx: &CallCtx, entries: &[repo::Entry]) -> Outcome {
    let surface = ctx.ui.table(
        ["commit", "subject", "author"],
        entries
            .iter()
            .map(|e| vec![e.sha.clone(), e.subject.clone(), e.author.clone()]),
    );
    Outcome::Ok {
        surface: Some(surface),
        value: Some(json!(entries.iter().map(entry_json).collect::<Vec<_>>())),
    }
}
