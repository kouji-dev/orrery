//! `orrery workflow check|run` — the product's way into plan 11.
//!
//! # Why the command tree needed a new verb
//!
//! `orrery run -p "…"` submits exactly one turn and has nowhere to put a
//! workflow file, so `orrery-orchestrator` and `orrery-router` were absent from
//! `cargo tree -p orrery-cli` altogether: the loop machine, its caps and every
//! routing rule were green library tests a person could not reach. A green test
//! the binary cannot reach does not count as done.
//!
//! # `check` starts nothing
//!
//! Asking whether a workflow is well-formed must work in a checkout with no
//! provider in sight, the same rule `config explain` and `mcp list` follow. It
//! is also the surface for translation #5 — an invalid workflow fails at load,
//! not after three model calls — because the typecheck is the whole of what
//! `check` does.

use std::path::Path;

use orrery_orchestrator::{Catalogue, Outcome, Workflow, WorkflowRun};
use tokio_util::sync::CancellationToken;

use crate::args::{Cli, WorkflowCommand};
use crate::exit::{Exit, fail};

/// Route the subcommand.
pub fn dispatch(cli: &Cli, command: &WorkflowCommand) -> ! {
    match command {
        WorkflowCommand::Check { file } => check(cli, file),
        WorkflowCommand::Run { file, max_tokens } => run(cli, file, *max_tokens),
    }
}

/// Read and typecheck one, exiting 2 with the load error when it will not.
///
/// The catalogue is empty, so a `ref` into a step's value is reported as
/// unchecked rather than refused: the agents and tools in force come from the
/// extensions a session loads, and `check` deliberately loads none. What it
/// still catches is everything structural — a forward reference, a duplicate
/// name, a loop with no `until`, a path into a shape that has no such key.
fn load(file: &Path) -> orrery_orchestrator::Checked {
    let text = std::fs::read_to_string(file)
        .unwrap_or_else(|e| fail(Exit::Usage, format!("{}: {e}", file.display())));
    Workflow::load(&text, &file.display().to_string(), &Catalogue::empty())
        .unwrap_or_else(|e| fail(Exit::Usage, e))
}

fn check(cli: &Cli, file: &Path) -> ! {
    let checked = load(file);
    let workflow = checked.as_workflow();
    if crate::cmd::layers::wants_json(cli) {
        let report = serde_json::json!({
            "workflow": workflow.name,
            "file": file.display().to_string(),
            "steps": workflow.steps.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            "checked": true,
        });
        println!("{report}");
    } else {
        println!(
            "{name}  {n} step(s)  ok",
            name = workflow.name,
            n = workflow.steps.len()
        );
        for step in &workflow.steps {
            println!("  {}  {}", step.name, step.step.kind());
        }
    }
    Exit::Ok.exit()
}

fn run(cli: &Cli, file: &Path, max_tokens: Option<u64>) -> ! {
    let checked = load(file);
    let session = crate::cmd::session(cli);
    let harness = session.harness();
    let handle = session.handle();

    let budget = max_tokens.map(|max_tokens| orrery_proto::Budget {
        max_tokens,
        ..checked.as_workflow().budget
    });
    let run = handle.block_on(harness.run_workflow(&checked, budget, CancellationToken::new()));

    if crate::cmd::layers::wants_json(cli) {
        println!("{}", report(&run));
    } else {
        human(&run);
    }
    // A capped loop is an ordinary ending, so it is 0. Only an outcome that
    // means the work did not get done is not.
    match &run.outcome {
        Outcome::Completed => Exit::Ok.exit(),
        Outcome::StoppedByBudget { .. } => Exit::Budget.exit(),
        // A gate that stopped or escalated, and a step that failed, all mean
        // the same thing to a CI script: it ran, and the task did not pass.
        _ => Exit::TaskFailed.exit(),
    }
}

/// One JSON object per run: what it came to, what every step returned, what it
/// spent, and every loop that ended on its cap.
fn report(run: &WorkflowRun) -> serde_json::Value {
    let mut out = serde_json::json!({
        "outcome": word(&run.outcome),
        "steps_run": run.steps_run,
        "usage": run.usage,
        "caps": run.caps.iter().map(|c| serde_json::json!({
            "step": c.step,
            "iterations": c.iterations,
        })).collect::<Vec<_>>(),
        "steps": serde_json::to_value(&run.env).unwrap_or(serde_json::Value::Null),
    });
    // The detail of *which* ending, beside the word for it. A client that only
    // reads `outcome` still works; one that wants the reason has it.
    match &run.outcome {
        Outcome::StoppedByGate { step } | Outcome::Escalated { step } => {
            out["step"] = serde_json::json!(step);
        }
        Outcome::StoppedByBudget { kind } => {
            out["kind"] = serde_json::json!(format!("{kind:?}").to_lowercase());
        }
        Outcome::Failed {
            step,
            code,
            message,
        } => {
            out["step"] = serde_json::json!(step);
            out["code"] = serde_json::json!(code);
            out["message"] = serde_json::json!(message);
        }
        _ => {}
    }
    out
}

fn word(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Completed => "completed",
        Outcome::StoppedByGate { .. } => "stopped-by-gate",
        Outcome::Escalated { .. } => "escalated",
        Outcome::StoppedByBudget { .. } => "stopped-by-budget",
        Outcome::Failed { .. } => "failed",
        Outcome::Cancelled => "cancelled",
        // `Outcome` is `#[non_exhaustive]`. An ending this build has not been
        // taught about still has to be reported as something.
        _ => "unknown",
    }
}

fn human(run: &WorkflowRun) {
    println!("{}  {} step(s) run", word(&run.outcome), run.steps_run);
    for cap in &run.caps {
        println!(
            "  `{step}` stopped on its cap after {n} iteration(s)",
            step = cap.step,
            n = cap.iterations
        );
    }
    if let Outcome::Failed {
        step,
        code,
        message,
    } = &run.outcome
    {
        println!("  `{step}` failed — {code}: {message}");
    }
    println!(
        "  {} token(s)",
        run.usage.input_tokens + run.usage.output_tokens
    );
}
