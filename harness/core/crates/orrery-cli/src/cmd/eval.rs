//! `orrery eval` - run, compare and replay evaluation suites.
//!
//! # Three functions of wiring, and what they wire
//!
//! `orrery-eval` shipped complete — isolation, the matrix, the runner, the
//! graders, compare, replay, JUnit — and reached none of it from the binary.
//! This module is the wiring, and the only decisions it makes are the ones a
//! command line has to make:
//!
//! - **Where a run's report lives.** `<state-dir>/eval/<run-id>.json`, written
//!   by `run` and read by `compare` and `replay`. Without somewhere to put it,
//!   `compare` has nothing to take arguments *about*: a run id has to name
//!   something.
//! - **Which store the cases share.** One session database under the state
//!   directory, so `eval replay` can re-open a case's transcript afterwards.
//!   The *workspaces* are still one per case and still deleted on drop — that
//!   is `Isolator`'s, and nothing here relaxes it.
//! - **Which profile a runner is bound to.** The matrix selects runners by
//!   profile name; this build binds every named profile to the same fixture
//!   runner, because the fixture provider is the only one it can select. The
//!   day a real provider is selectable, this is where the binding changes and
//!   nothing else moves.
//!
//! # No network, no key
//!
//! The model is a committed `.jsonl` replayed by the fixture provider, exactly
//! as `orrery run`'s is. The judge grader is **not** installed: it costs money,
//! and a build that cannot pay for it should not offer it.
//!
//! Implementation plan: `harness/docs/plans/16-eval-runner.md` (the runner) and
//! `harness/docs/plans/17-cli.md` task 9 (the command).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_broker::LocalBroker;
use orrery_eval::{
    EvalRun, EvalRunner, HarnessRunner, Isolator, Matrix, RoleBinding, RunReport, Suite,
};
use orrery_harness::PolicyBroker;
use orrery_proto::{Role, Subject};

use crate::args::{Cli, EvalCommand};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch an `eval` subcommand.
pub fn dispatch(cli: &Cli, command: &EvalCommand) -> ! {
    match command {
        EvalCommand::Run {
            suite,
            profile,
            model,
            format,
        } => run(cli, suite, profile, model, format.as_deref()),
        EvalCommand::Compare { run_a, run_b } => compare(cli, run_a, run_b),
        EvalCommand::Replay { run, case } => replay(cli, run, case),
    }
}

/// Where a run's report is written, and where `compare` and `replay` look.
fn reports_dir(cli: &Cli) -> PathBuf {
    crate::cmd::session::state_dir(cli).join("eval")
}

/// Read a report by run id, or by a path to one.
///
/// Both, because a person who has the file in front of them should not have to
/// learn where the harness filed it.
fn report(cli: &Cli, which: &str) -> RunReport {
    let direct = PathBuf::from(which);
    let path = if direct.is_file() {
        direct
    } else {
        reports_dir(cli).join(format!("{which}.json"))
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!("no run `{which}` ({}): {e}", path.display()),
        )
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!("{} is not a run report: {e}", path.display()),
        )
    })
}

/// Run a suite.
fn run(cli: &Cli, suite: &str, profiles: &[String], models: &[String], format: Option<&str>) -> ! {
    let suite = load_suite(suite);
    let matrix = Matrix::new(profiles.to_vec(), models.to_vec());
    let points = matrix.expand();

    // One kernel's worth of wiring — a store, a provider, a policy engine — and
    // then the eval runner over it. `cmd::session` is what refuses when no
    // provider was named, with the same message `run` gives.
    let session = crate::cmd::session(cli);
    let harness = session.harness();
    let store = harness.store().clone();
    let engine = harness.engine().clone();

    let provider = orrery_harness::features::fixture_provider(&crate::cmd::setup(cli).fixtures)
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    let broker = PolicyBroker::new(
        engine.clone(),
        Arc::new(LocalBroker::new(engine.ledger().clone())),
        crate::cmd::setup(cli).workspace,
        Subject::Agent,
        harness.scope().clone(),
        orrery_harness::default_tool_budget(),
    );

    let isolator = Isolator::new(reports_dir(cli).join("workspaces"));
    let mut runner = EvalRunner::new(isolator);
    for grader in orrery_harness::features::graders(broker, Some(store.clone()), None) {
        runner = runner.with_grader(grader);
    }
    // Every point of the matrix is bound to the same runner in this build: the
    // fixture provider is the only selectable one, so a second profile is a
    // second *label*, not a second model. Said out loud rather than implied,
    // because a matrix whose points are secretly identical is a matrix that
    // reports agreement it did not measure.
    for point in &points {
        let mut bindings = HashMap::new();
        bindings.insert(
            Role::Executor,
            RoleBinding {
                provider: provider.clone(),
                model: point.model.clone(),
            },
        );
        runner = runner.with_runner(
            point.profile.clone(),
            Arc::new(HarnessRunner::new(
                point.profile.clone(),
                store.clone(),
                bindings,
            )),
        );
    }

    let spec = EvalRun::new(suite.name.clone(), matrix);
    let report = harness
        .block_on(runner.run(&spec, &suite))
        .unwrap_or_else(|e| fail(Exit::Kernel, e));

    save(cli, &report);
    emit(cli, &report, format);
    // A red suite exits non-zero, with or without a baseline. `exit_code`
    // decides that, so CI and this command cannot disagree.
    std::process::exit(orrery_eval::exit_code(&report, None))
}

/// Write the report where `compare` and `replay` will find it.
fn save(cli: &Cli, report: &RunReport) {
    let dir = reports_dir(cli);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        fail(Exit::Kernel, format!("{}: {e}", dir.display()));
    }
    let path = dir.join(format!("{}.json", report.run_id));
    let text = report
        .to_json()
        .unwrap_or_else(|e| fail(Exit::Kernel, format!("could not render the report: {e}")));
    if let Err(e) = std::fs::write(&path, text) {
        fail(Exit::Kernel, format!("{}: {e}", path.display()));
    }
    // stdout is data: the report itself goes there, and where it was filed is
    // narration.
    eprintln!("orrery: {} written to {}", report.run_id, path.display());
}

/// The report, in the shape that was asked for.
fn emit(cli: &Cli, report: &RunReport, format: Option<&str>) {
    let format = format.unwrap_or(if layers::wants_json(cli) {
        "json"
    } else {
        "text"
    });
    match format {
        // One object on one line, like every other `--json` surface in this
        // binary: `RunReport::to_json` pretty-prints, which is right for the
        // file on disk and wrong for a pipe into `jq`.
        "json" => match serde_json::to_string(report) {
            Ok(text) => println!("{text}"),
            Err(e) => fail(Exit::Kernel, format!("could not render the report: {e}")),
        },
        "junit" => println!("{}", orrery_eval::junit_xml(report)),
        "text" => {
            println!("{}  {}", report.run_id, report.suite);
            println!("{}", report.reproducibility.describe());
            for r in &report.results {
                let score = r.score.map_or_else(String::new, |s| format!("  {s:.2}"));
                println!(
                    "{:?}  {}  {} turns, {} tool calls{score}",
                    r.outcome,
                    r.key(),
                    r.turns,
                    r.tool_calls
                );
            }
            println!("{}/{} passed", report.passed(), report.results.len());
        }
        other => fail(
            Exit::Usage,
            format!("`{other}` is not a report format: expected `text`, `json` or `junit`"),
        ),
    }
}

/// Compare two runs.
fn compare(cli: &Cli, run_a: &str, run_b: &str) -> ! {
    let before = report(cli, run_a);
    let after = report(cli, run_b);
    // `force` is false: two runs that pinned different things are not
    // comparable, and this command has no flag that says otherwise, because a
    // flag that says otherwise belongs where somebody can explain themselves.
    let comparison =
        orrery_eval::compare(&before, &after, false).unwrap_or_else(|e| fail(Exit::Usage, e));

    if layers::wants_json(cli) {
        match serde_json::to_string(&comparison) {
            Ok(text) => println!("{text}"),
            Err(e) => fail(Exit::Kernel, format!("could not render: {e}")),
        }
    } else {
        println!("{}", comparison.render());
    }
    if comparison.has_regressions() {
        Exit::TaskFailed.exit();
    }
    Exit::Ok.exit()
}

/// Re-open one case of a past run.
fn replay(cli: &Cli, run: &str, case: &str) -> ! {
    let report = report(cli, run);
    let Some(store) = crate::cmd::session::store(cli) else {
        fail(
            Exit::Usage,
            format!(
                "no sessions in {}",
                crate::cmd::session::state_dir(cli).display()
            ),
        );
    };
    let replayed = crate::cmd::session::runtime()
        .block_on(orrery_eval::replay(&store, &report, case))
        .unwrap_or_else(|e| fail(Exit::Usage, e));

    if layers::wants_json(cli) {
        println!(
            "{}",
            serde_json::json!({
                "case": replayed.case,
                "profile": replayed.profile,
                "events": replayed.len(),
                "messages": replayed.messages,
            })
        );
        Exit::Ok.exit();
    }

    println!("case    {}", replayed.case);
    println!("profile {}", replayed.profile);
    println!("events  {}", replayed.len());
    println!();
    for message in &replayed.messages {
        let role = format!("{:?}", message.role).to_lowercase();
        for block in &message.content {
            if let orrery_proto::ContentBlock::Text { text } = block {
                println!("{role}: {text}");
            } else {
                println!("{role}: [block]");
            }
        }
    }
    Exit::Ok.exit()
}

/// A suite, by path.
///
/// A bare name is resolved under `<workspace>/evals/<name>.toml`, so a
/// repository can keep its suites where a reader would look for them and
/// `orrery eval run smoke` works without a path.
fn load_suite(which: &str) -> Suite {
    let direct = Path::new(which);
    let path = if direct.is_file() {
        direct.to_path_buf()
    } else {
        PathBuf::from("evals").join(format!("{which}.toml"))
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!("no suite `{which}` ({}): {e}", path.display()),
        )
    });
    Suite::from_toml(&text).unwrap_or_else(|e| fail(Exit::Usage, e))
}
