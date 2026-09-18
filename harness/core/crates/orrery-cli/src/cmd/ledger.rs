//! `orrery ledger` and `orrery telemetry` - the audit streams, queryable.
//!
//! # The operator surface section 8 phase 3 asks for
//!
//! "Every decision is logged" is a claim about a file. It is only true if
//! somebody can open that file and ask it a question, and until this command
//! landed nobody could: `orrery-audit` wrote a hashed, append-only JSONL stream
//! and had no reader at all. This is the reader.
//!
//! # Two commands, one scan
//!
//! `ledger` and `telemetry` differ by exactly one filter — the stream an event
//! declares itself to belong to, via `AuditEvent::stream()`. The evidence
//! (decisions, calls, consent) and the counts (model requests, routing) are
//! written into the same file in the same order, because splitting them at
//! write time would mean deciding at write time which one a new variant is.
//!
//! # One file per session
//!
//! A run writes `<state-dir>/audit/<session>.jsonl`; see
//! [`audit_dir`](crate::session::audit_dir). So `--session` is a file open, and
//! no `--session` is every file, oldest first. The events carry no session id
//! of their own on purpose: the schema is about what was decided, and a second
//! identity for what the file name already says is a second thing to keep
//! consistent.
//!
//! # No kernel, no provider
//!
//! Reading what happened must not need a model, exactly as `session list` must
//! not. Nothing here builds a kernel.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`, rendered
//! by `harness/docs/plans/17-cli.md` task 8.

use std::path::PathBuf;

use orrery_audit::layer::Stream;
use orrery_audit::{AuditEvent, AuditRecord, Query, Scan};
use orrery_proto::Subject;

use crate::args::Cli;
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Query the decision ledger.
pub fn ledger(
    cli: &Cli,
    session: Option<&str>,
    subject: Option<&str>,
    rule: Option<&str>,
    limit: Option<usize>,
) -> ! {
    let subject = subject.map(|s| {
        s.parse::<Subject>()
            .unwrap_or_else(|e| fail(Exit::Usage, e))
    });
    let query = Query {
        stream: Some(Stream::Audit),
        subject,
        rule: rule.map(str::to_owned),
        limit,
    };
    report(cli, session, &query)
}

/// Query the telemetry stream.
pub fn telemetry(cli: &Cli, session: Option<&str>, limit: Option<usize>) -> ! {
    let query = Query {
        stream: Some(Stream::Telemetry),
        limit,
        ..Query::default()
    };
    report(cli, session, &query)
}

/// Scan the selected streams and print what matched.
fn report(cli: &Cli, session: Option<&str>, query: &Query) -> ! {
    let dir = crate::session::audit_dir(&crate::cmd::session::state_dir(cli));
    let files = files(&dir, session);
    if files.is_empty() {
        eprintln!("orrery: nothing recorded in {}", dir.display());
        Exit::Ok.exit();
    }

    let json = layers::wants_json(cli);
    let mut skipped = 0usize;
    let mut shown = 0usize;
    for file in &files {
        let found: Scan = orrery_audit::scan(file, query)
            .unwrap_or_else(|e| fail(Exit::Kernel, format!("{}: {e}", file.display())));
        skipped += found.skipped;
        for record in &found.records {
            shown += 1;
            if json {
                match serde_json::to_string(record) {
                    Ok(line) => println!("{line}"),
                    Err(e) => fail(Exit::Kernel, format!("could not render a record: {e}")),
                }
            } else {
                println!("{}", line(record));
            }
        }
    }

    // Narration on stderr, so `--json` stdout stays parseable. A stream with an
    // unreadable line says so rather than quietly showing fewer records.
    if shown == 0 {
        eprintln!("orrery: nothing matched");
    }
    if skipped > 0 {
        eprintln!("orrery: {skipped} unreadable line(s) in the stream");
    }
    Exit::Ok.exit()
}

/// Which files to scan: the one the session names, or all of them.
///
/// Sorted, so two runs of the same command print the same order. A
/// `--session` naming a stream that is not there is the person's mistake and
/// exits 2 rather than printing nothing and claiming success.
fn files(dir: &std::path::Path, session: Option<&str>) -> Vec<PathBuf> {
    if let Some(session) = session {
        let path = dir.join(format!("{session}.jsonl"));
        if !path.exists() {
            fail(
                Exit::Usage,
                format!("no audit stream for session {session} in {}", dir.display()),
            );
        }
        return vec![path];
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    files.sort();
    files
}

/// One record, as a line a person reads.
fn line(record: &AuditRecord) -> String {
    match &record.event {
        AuditEvent::CapabilityDecision {
            subject,
            request,
            verdict,
            rule_text,
            layer,
            ..
        } => {
            let mut out = format!("{:<5} {subject} {request}", word(*verdict));
            if let Some(text) = rule_text {
                out.push_str(&format!("  <- {text}"));
            }
            if let Some(layer) = layer {
                out.push_str(&format!(" [{layer:?}]"));
            }
            out
        }
        AuditEvent::ToolCall {
            tool, outcome, call, ..
        } => format!("call  {tool} {call} {outcome:?}"),
        AuditEvent::ToolName {
            name,
            candidates,
            chose,
        } => format!("name  {name} -> {chose} (of {})", candidates.len()),
        AuditEvent::ConsentAnswer { prompt, answer } => format!("ask   {prompt} {answer}"),
        AuditEvent::SubAgentSpawn { parent, agent } => format!("spawn {parent} -> {agent}"),
        AuditEvent::ModelRequest {
            model,
            input_tokens,
            output_tokens,
        } => format!("model {model} in={input_tokens} out={output_tokens}"),
        AuditEvent::RoutingDecision { chose, signals } => {
            format!("route {chose} ({} signals)", signals.len())
        }
        AuditEvent::ExtensionLoad { ext, status, .. } => format!("load  {ext} {status}"),
        AuditEvent::Content { action, .. } => format!("content {action}"),
        // `AuditEvent` is `#[non_exhaustive]`. A variant this build does not
        // know how to render is still evidence; print its tag.
        other => format!("?     {:?}", other.stream()),
    }
}

/// The verdict, in a fixed-width word so the column lines up.
fn word(verdict: orrery_audit::Verdict) -> &'static str {
    match verdict {
        orrery_audit::Verdict::Allow => "allow",
        orrery_audit::Verdict::Ask => "ask",
        orrery_audit::Verdict::Deny => "deny",
        // `Verdict` is `#[non_exhaustive]`; an unknown one is still a decision.
        _ => "?",
    }
}
