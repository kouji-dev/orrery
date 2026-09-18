//! Task 8, invariant 3 · **phases fire inside every step regardless of the
//! agent**, so an interceptor written once applies to all of them.
//!
//! It lives here rather than in `orrery-router` because steps run here: the
//! router decides and returns data, and letting it reach the loop would be the
//! kernel dependency the whole design is arranged to avoid.
//!
//! The per-*pass* phase pipeline is plan 05's, inside the kernel. This is the
//! step-level half of the same invariant, and the half this crate owns.

mod common;

use std::sync::{Arc, Mutex};

use common::Fake;
use orrery_orchestrator::Catalogue;
use orrery_orchestrator::typecheck::Workflow;
use orrery_orchestrator::workflow::{Outcome, Runner, StepCtx, StepObserver};
use orrery_proto::Budget;

/// One interceptor, registered once, writing down everything it sees.
#[derive(Default)]
struct Watcher {
    seen: Mutex<Vec<(String, Option<String>)>>,
}

impl Watcher {
    fn seen(&self) -> Vec<(String, Option<String>)> {
        self.seen.lock().expect("not poisoned").clone()
    }
}

impl StepObserver for Watcher {
    fn entered(&self, ctx: &StepCtx) {
        self.seen
            .lock()
            .expect("not poisoned")
            .push((ctx.step.clone(), ctx.agent.clone()));
    }
}

const TWO_ROLES: &str = r#"
name = "plan-then-do"

[[step]]
name = "think"
kind = "agent"
subagent = "planner"
input = "work out what to change"

[[step]]
name = "check"
kind = "tool"
ref = "fs.read"
input = "./src"

[[step]]
name = "do"
kind = "agent"
subagent = "executor"
input = "change it"

[[step]]
name = "again"
kind = "loop"
max_iterations = 2
until = { any = [] }

[[step.again.body]]
name = "unused"
kind = "tool"
ref = "fs.read"
input = "./src"
"#;

#[tokio::test]
async fn phases_fire_in_every_step() {
    let exec = Fake::new().costing(10);
    // Registered **once**, on the runner, and never per agent.
    let watcher = Arc::new(Watcher::default());
    let workflow = Workflow::load(
        // The nested-body spelling above is awkward in TOML; write the loop's
        // body the ordinary way.
        &TWO_ROLES.replace("[[step.again.body]]", "[[step.body]]"),
        "wf.toml",
        &Catalogue::empty(),
    )
    .expect("it loads");

    let run = Runner::new(&exec)
        .observed_by(Arc::clone(&watcher) as Arc<dyn StepObserver>)
        .run(
            &workflow,
            Some(Budget {
                max_turns: 100,
                max_tokens: 1_000_000,
                wall_clock_ms: 600_000,
                max_micro_usd: None,
            }),
        )
        .await;
    assert_eq!(run.outcome, Outcome::Completed);

    let seen = watcher.seen();
    let agents: Vec<&str> = seen
        .iter()
        .filter_map(|(_, agent)| agent.as_deref())
        .collect();
    assert!(
        agents.contains(&"planner") && agents.contains(&"executor"),
        "one interceptor observed passes under the planner *and* the executor: {seen:?}"
    );

    // And under no agent at all: a tool step and a loop are steps like any
    // other, so a step-level interceptor is not something an author has to
    // register per role.
    let steps: Vec<&str> = seen.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        steps,
        vec!["think", "check", "do", "again", "unused", "unused"],
        "every step, in order, including both times round the loop"
    );
}

#[tokio::test]
async fn an_observer_sees_which_time_round_a_loop_is() {
    #[derive(Default)]
    struct Rounds(Mutex<Vec<u32>>);
    impl StepObserver for Rounds {
        fn entered(&self, ctx: &StepCtx) {
            if ctx.kind == "tool" {
                self.0.lock().expect("not poisoned").push(ctx.iteration);
            }
        }
    }

    let exec = Fake::new();
    let rounds = Arc::new(Rounds::default());
    let workflow = Workflow::load(
        &TWO_ROLES.replace("[[step.again.body]]", "[[step.body]]"),
        "wf.toml",
        &Catalogue::empty(),
    )
    .expect("it loads");

    Runner::new(&exec)
        .observed_by(Arc::clone(&rounds) as Arc<dyn StepObserver>)
        .run(
            &workflow,
            Some(Budget {
                max_turns: 100,
                max_tokens: 1_000_000,
                wall_clock_ms: 600_000,
                max_micro_usd: None,
            }),
        )
        .await;

    assert_eq!(
        *rounds.0.lock().expect("not poisoned"),
        vec![0, 1, 2],
        "the step outside the loop, then round one, then round two"
    );
}
