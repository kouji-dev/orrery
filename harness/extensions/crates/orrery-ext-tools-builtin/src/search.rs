//! `grep` and `glob` — the two that walk a tree.
//!
//! # The honest part
//!
//! Discovery is `std::fs::read_dir`. The broker facade has five methods and
//! none of them lists a directory, so a walk cannot go through it; see the
//! crate docs. Every **byte** still does: `grep` reads each candidate through
//! [`BrokerFacade::read`](orrery_ext_api::BrokerFacade::read) under the call's
//! ceiling, so a 10 MB file costs the ceiling and not 10 MB, and a `read` the
//! policy refuses is refused here too.

use std::path::{Path, PathBuf};

use orrery_ext_api::{CallCtx, ReadRequest};
use orrery_proto::Outcome;
use serde_json::Value;

use crate::{bad_input, string_arg, text_outcome};

/// How many entries a walk will look at before it stops.
///
/// A walk is a tool call, and a tool call has a ceiling. Without this, a glob
/// over `/` is bounded only by patience.
const MAX_ENTRIES: usize = 20_000;

/// How many matches `grep` reports when nobody says otherwise.
const DEFAULT_MAX_MATCHES: usize = 200;

/// Where to start walking: the `path` argument, or the process's directory.
fn root(input: &Value) -> PathBuf {
    input
        .get("path")
        .and_then(Value::as_str)
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// Every file under `root`, breadth-first, capped.
///
/// Errors are skipped rather than raised: a directory that cannot be listed is
/// one this call cannot see, which is the same answer a `read` deny gives.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => queue.push(path),
                Ok(t) if t.is_file() => out.push(path),
                _ => {}
            }
            if out.len() >= MAX_ENTRIES {
                return out;
            }
        }
    }
    out.sort();
    out
}

/// Search a tree, bounded per file.
pub(crate) async fn grep(input: Value, ctx: &CallCtx) -> Result<Outcome, Outcome> {
    let pattern = string_arg(&input, "pattern")?;
    let root = root(&input);
    let max = input
        .get("max_matches")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_MAX_MATCHES, |n| n as usize);

    let mut lines: Vec<String> = Vec::new();
    let mut truncated_files: Vec<String> = Vec::new();
    for file in walk(&root) {
        if lines.len() >= max {
            break;
        }
        let Ok(chunk) = ctx
            .broker
            .read(ReadRequest::new(&file, ctx.budget.output_bytes))
            .await
        else {
            // A file this call may not read is a file this call does not
            // search. The denial is already in the audit.
            continue;
        };
        if !chunk.eof {
            truncated_files.push(display(&file, &root));
        }
        let text = String::from_utf8_lossy(&chunk.bytes);
        for (n, line) in text.lines().enumerate() {
            if line.contains(&pattern) {
                lines.push(format!(
                    "{}:{}: {}",
                    display(&file, &root),
                    n + 1,
                    line.trim_end()
                ));
                if lines.len() >= max {
                    break;
                }
            }
        }
    }

    let mut report = if lines.is_empty() {
        format!("no match for `{pattern}`")
    } else {
        lines.join("\n")
    };
    if !truncated_files.is_empty() {
        report.push_str(&format!(
            "\n[only the first {} bytes of {} file(s) were searched: {}]",
            ctx.budget.output_bytes,
            truncated_files.len(),
            truncated_files.join(", ")
        ));
    }
    Ok(text_outcome(ctx, report))
}

/// List the files matching a glob.
pub(crate) async fn glob(input: Value, ctx: &CallCtx) -> Result<Outcome, Outcome> {
    let pattern = string_arg(&input, "pattern")?;
    let root = root(&input);
    let matcher = globset::Glob::new(&pattern)
        .map_err(|e| bad_input(format!("`{pattern}` is not a glob: {e}")))?
        .compile_matcher();

    let mut hits: Vec<String> = walk(&root)
        .into_iter()
        .map(|p| display(&p, &root))
        .filter(|rel| matcher.is_match(rel))
        .collect();
    hits.sort();

    Ok(text_outcome(
        ctx,
        if hits.is_empty() {
            format!("nothing matches `{pattern}`")
        } else {
            hits.join("\n")
        },
    ))
}

/// A path as the caller will recognise it: relative to the root it asked
/// about, with forward slashes so a glob means the same thing on every
/// platform.
fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
