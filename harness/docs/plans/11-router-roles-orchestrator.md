# 11 · Router, roles and orchestrator — escalation as a decision, not a vibe

**Goal.** The model proposes, the router disposes. Escalation from one pass to a bounded loop to a sub-agent to five sub-agents to a workflow is a request checked against declared rules and a budget, audited with the signal values behind it. Roles are named by the kernel and bound by config, so "plan with a large model, compact with a cheap one" is configuration. And every loop, everywhere, declares a termination predicate and a hard cap the kernel enforces. When this is done, a verify loop terminates on its own cap.

**Covers.** §4.6's routing, rungs, fan-out, modes and roles · §4.10 in full.

**Crates.** `core/crates/orrery-router` · `core/crates/orrery-orchestrator` · `extensions/crates/orrery-ext-agents-default`.

**Depends on.** [`02`](02-session-store.md) (branches and leases), [`05`](05-kernel-loop.md) (the loop it routes), [`07`](07-policy-broker-audit.md) (modes are permissions), [`10`](10-config-layers.md) (bindings).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **`Router::decide` is sync and returns data.** That is the cut that keeps the dependency graph acyclic: the kernel runs a turn, the orchestrator runs steps, and the router only produces a `RouteDecision` for the orchestrator to act on.
- **Nothing in the system is a free-running `while`.** Every loop declares a predicate and a hard cap.
- A role binding is **intersected** with the step's grant and never widens it.
- A child branch's result is merged **by the parent at the join** (plan 02).
- `budget` on an agent is mandatory — an agent that cannot terminate is a cost incident.

This plan owns translation **#5** (`Expr` typechecked at load) and translation **#4** jointly with plan 06.

---

## Architecture

### Phases are when; agents are who

A **phase** is a point inside a pass where a deterministic verdict fits. An **agent** is a configuration of the loop — prompt, model, tools, permissions, budget. A **step** is a span running under one agent. So the loop is a sequence of steps, each bound to an agent, and every pass inside a step goes through the same phases whoever is acting.

An agent is a configuration of the loop, **never a second loop.**

### The rungs

Climbed one at a time; skipping one needs an explicit instruction.

| Rung | Justified when | Bounded by |
|---|---|---|
| One pass | default | the turn budget |
| Bounded loop | a verifiable failure signal exists (tests, a gate) | `max_iterations` + predicate |
| Sub-agent | the work needs a fresh context or narrower capabilities | a slice of the parent budget |
| Parallel sub-agents | inputs decompose into demonstrably disjoint sets | the profile's fan-out cap |
| Workflow | the sequence is known ahead and deterministic steps can replace model calls | the whole-workflow budget |

### Precedence, and cheap signals only

An explicit user instruction, then a declared rule, then a model proposal the router grants, downgrades or denies.

Rules read **cheap signals only**: budget spent, turns in the current mode, read/write mix, diff size, gate and test outcomes, repeated identical calls. Nothing that costs a model call.

```rust
pub struct Signals {
    pub budget_spent: Usage, pub budget_limit: Budget,
    pub turns_in_mode: u32, pub mode: Mode,
    pub reads: u32, pub writes: u32, pub diff_lines: u32,
    pub last_gate: Option<GateOutcome>, pub repeated_identical_calls: u32,
}

/// Sync. Pure. Returns DATA — this is what breaks the kernel↔orchestrator cycle.
pub fn decide(&self, s: &Signals, scope: &AgentScope, proposal: Option<Proposal>) -> RouteDecision;

#[non_exhaustive]
pub enum RouteDecision {
    NextPass,
    BoundedLoop { body: StepRef, until: Predicate, max_iterations: u32 },
    SubAgent    { agent: String, budget: Budget },
    FanOut      { agent: String, inputs: Vec<serde_json::Value>, budget_each: Budget },
    Workflow    { name: String },
    Deny        { reason: String },
}
```

Every decision is audited **with the signal values behind it**, so "why five and not two" is answerable afterwards.

### Fan-out is computed, not chosen

```
N = min(disjoint units the parent can name,
        the profile's fan-out cap,
        remaining budget ÷ fanout.child_cost)
```

`child_cost` is a **declared profile value**, not a model guess and not an after-the-fact estimate. Where a profile declares none, the cap and the disjoint units bound N on their own.

Independence is demonstrated by giving each child a **disjoint input set**; overlapping sets mean one problem and one agent. This is bounded delegation, not racing five agents at one problem.

### Modes are permissions

`plan` is read-only, `execute` is the full granted set, `review` never writes. Because `mode(plan)` and `mode(execute)` are grantable like anything else (§4.8), "this profile may never enter execute" is one line, and a mode switch is an ordinary capability request the policy engine answers — **not a UI state**.

### Roles

The kernel names them; extensions and profiles bind them. Same indirection as the provider layer, turned on the loop itself.

| Role | Runs at | Default |
|---|---|---|
| `planner` | a step in `plan` mode | ships, read-only tools |
| `executor` | a step in `execute` mode | ships, the granted set |
| `verifier` | the gate of a bounded loop | ships |
| `compactor` | the `compact` phase | ships, cheap model |
| `summariser` | a sub-agent's `returns: "summary"`, session titles | ships, cheap model |
| `router` | the escalation decision | **unbound — declarative rules** |
| `grader` | eval judging | per suite |

Four invariants keep this from becoming a second control flow, each a test:

1. A role binding is **intersected** with the step's grant and never widens it.
2. Exactly **one** binding wins per role per step, by layer precedence, and the audit records which one ran.
3. Phases fire inside **every** step regardless of the agent, so an interceptor written once applies to all of them.
4. A role bound to an agent that does not exist fails at **`session.start`**, not mid-turn.

Leaving `router` unbound keeps routing declarative and reproducible — which is what eval comparison needs. Binding it to an agent is how a model router happens, and it is the same mechanism, not a new one.

### Workflows

```rust
#[non_exhaustive]
pub enum Step {
    Agent    { subagent: String, input: Expr },
    Tool     { r#ref: String, input: Expr },                      // no model call at all
    Parallel { steps: Vec<Step>, join: Join },                    // All | First | Quorum(u32)
    Loop     { body: Vec<Step>, until: Predicate, max_iterations: u32 },
    Gate     { check: Predicate, on_fail: OnFail },               // Stop | Retry | Escalate
}
```

Deterministic steps between model calls are **where cost comes out**, since they cost nothing.

**Translation #5: the whole dataflow is typechecked at load.** Step *N* may only `ref` steps `< N`, and each ref's path must typecheck against the target's declared `returns`. A workflow must not fail mid-run on a type error after paying for three model calls.

And it is the **compiler** that says so, not API discipline. `Workflow` parses; `Checked` is a newtype whose field is private and whose only constructor runs the typechecker, and `Runner::run` takes `&Checked`. `Workflow::from_toml_str` still exists and still does not check — it simply produces something the runner will not accept, and editing a loaded workflow means checking it again. A `compile_fail` doctest on `Checked` is the proof.

---

## File structure

**Create**

- `harness/core/crates/orrery-router/src/{lib,signals,rules,decide,fanout,mode}.rs`
- `harness/core/crates/orrery-router/tests/{decide,fanout,rules,mode,roles}.rs` — one file per task, so a failure names the task
- `harness/core/crates/orrery-router/src/roles.rs` — role binding, which Task 8 puts in `lib.rs`; it is a module of its own, re-exported from there
- `harness/core/crates/orrery-orchestrator/src/{lib,step,workflow,subagent,expr,typecheck,budget,join}.rs`
- `harness/core/crates/orrery-orchestrator/tests/{workflow,subagent,loops,typecheck,expr,roles}.rs`, plus `tests/common/` — a recording `StepExecutor` and a `Vec`-backed session store, so **no test here needs a provider, a model or a network**
- `harness/extensions/crates/orrery-ext-agents-default/{Cargo.toml,orrery.toml,README.md,src/lib.rs}`

---

## Tasks

### Task 1 · Signals and declarative rules

Files: `orrery-router/src/{signals,rules}.rs`

- [x] **Failing test first.** `rules::signals_are_cheap` — a compile-level check that `Signals` contains no handle, no future and nothing requiring a model call; plus a doc note listing what is deliberately absent.
- [x] `rules::parse_from_config`.
- [x] Implement.

### Task 2 · `decide`

Files: `orrery-router/src/decide.rs`, `tests/decide.rs`

- [x] **Failing test first.** `decide::user_instruction_beats_a_rule` — explicit instruction wins over a declared rule that says otherwise.
- [x] `decide::rule_beats_a_model_proposal` — a proposal to fan out where a rule forbids it is denied.
- [x] `decide::rungs_are_climbed_one_at_a_time` — a proposal jumping from one pass to a workflow is downgraded unless explicitly instructed.
- [x] `decide::is_pure` — same signals in, same decision out, 1000 times.
- [x] `decide::is_audited_with_signal_values` — the audit entry contains the numbers that produced the decision.
- [x] Implement.

### Task 3 · Fan-out arithmetic

Files: `orrery-router/src/fanout.rs`, `tests/fanout.rs`

- [x] **Failing test first.** `fanout::n_is_the_minimum` (proptest) — for arbitrary units, caps and budgets, N never exceeds any of the three bounds.
- [x] `fanout::overlapping_inputs_collapse_to_one` — inputs that are not disjoint produce N = 1.
- [x] `fanout::no_child_cost_still_bounded` — with no declared `child_cost`, the cap and units still bound N.
- [x] Implement.

### Task 4 · Modes as permissions

Files: `orrery-router/src/mode.rs`

- [x] **Failing test first.** `mode::switch_is_a_policy_question` — switching to `execute` without `mode(execute)` is denied by the policy engine, not by the router.
- [x] `mode::plan_is_read_only` — in `plan` mode, a write tool is not in `visible()`.
- [x] Implement.

### Task 5 · `Expr` evaluation and the load-time typecheck

Files: `orrery-orchestrator/src/{expr,typecheck}.rs`, `tests/typecheck.rs`

- [x] **Failing test first, and it is translation #5.** `typecheck::forward_ref_is_rejected_at_load` — step 1 referencing step 3 fails before anything runs, naming the file and step.
- [x] `typecheck::path_must_match_returns` — a ref to `.count` on a step whose `returns` has no `count` fails at load.
- [x] `typecheck::valid_workflow_passes`.
- [x] `expr::cannot_call_out` — a review-level assertion plus a test that the evaluator has no I/O in its signature.
- [x] `typecheck::only_the_typechecker_can_make_a_checked_workflow` — plus a `compile_fail` doctest on `Checked` proving an unchecked `Workflow` cannot be handed to the runner.
- [x] Implement.

### Task 6 · Sub-agents on branches

Files: `orrery-orchestrator/src/subagent.rs`, `tests/subagent.rs`

- [x] **Failing test first.** `subagent::grant_is_intersected` — a sub-agent declaring a wider grant than its parent gets the intersection; assert it cannot do what the parent could not.
- [x] `subagent::budget_is_mandatory` — an `AgentDefinition` without a budget fails to deserialize.
- [x] `subagent::runs_on_a_branch` — the sub-agent's turns are ordinary turns in the tree, inspectable, not collapsed into one tool result.
- [x] `subagent::parent_merges_at_the_join` — cross-reference plan 02 task 8; assert no deadlock and a `BranchResult` row.
- [x] `subagent::cannot_prompt_fails_typed` — a sub-agent that cannot prompt returns a typed error the parent handles, rather than auto-denying into a stall.
- [x] Implement.

### Task 7 · Workflows and loops

Files: `orrery-orchestrator/src/{workflow,join}.rs`, `tests/{workflow,loops}.rs`

- [x] **Failing test first, and it is the phase-6 criterion.** `loops::terminates_on_its_own_cap` — a verify loop whose gate never passes stops at `max_iterations` with a typed outcome, and the kernel enforced it rather than the prompt.
- [x] `loops::predicate_is_mandatory` — a `Loop` step without `until` fails at load. (`max_iterations` is mandatory too, and a cap of zero is a load error.)
- [x] `workflow::tool_step_makes_no_model_call` — assert the provider was never invoked.
- [x] `workflow::parallel_join_all_first_quorum` — three cases.
- [x] `workflow::gate_on_fail` — stop, retry and escalate.
- [x] `workflow::budget_is_whole_workflow` — the ceiling applies across steps, not per step.
- [x] Implement.

### Task 8 · Role binding

Files: `orrery-router/src/lib.rs`, `orrery-ext-agents-default/*`

- [x] **Failing test first.** `roles::missing_binding_fails_at_session_start` — a profile binding `planner` to a nonexistent agent fails at startup, naming the file and line, not mid-turn.
- [x] `roles::exactly_one_wins` — two extensions offering a planner; one is bound by precedence; the audit records which ran and the ledger names the loser.
- [x] `roles::phases_fire_in_every_step` — an interceptor registered once observes passes under the planner *and* the executor. **Lives in `orrery-orchestrator/tests/roles.rs`**, because steps run there and `orrery-router` must not gain a kernel dependency.
- [x] `roles::binding_never_widens`.
- [x] Implement the default agents as a real extension with a manifest.

---

## Done when

- `cargo test -p orrery-router -p orrery-orchestrator -p orrery-ext-agents-default` green. **26 + 29 tests + 2 doctests + 7 tests, all passing.**
- A verify loop terminates on its own cap, enforced by **the harness rather than the prompt** — specifically by `orrery-orchestrator`'s loop machine, on a counter it owns. *Amended*: the plan said "by the kernel", and it is not the kernel — `orrery-kernel` enforces a **turn** budget, and a step loop belongs to the orchestrator. The substance holds and is asserted in `loops::terminates_on_its_own_cap`: the body ran exactly `max_iterations` times and the model was handed the same input every time, so nothing in a prompt stopped it.
- An invalid workflow fails at load, not after three model calls — and unloadably so: `Runner::run` takes `Checked<Workflow>`, a newtype only `typecheck::check` can produce, so reaching the runner with an unchecked workflow is a compile error rather than a convention. Proved by the `compile_fail` doctest on `Checked` and by `typecheck::only_the_typechecker_can_make_a_checked_workflow`.
- A sub-agent's work is visible as ordinary turn **rows on its own branch** — `User` and `Assistant` turns, inspectable and replayable, never collapsed into one tool result. *Amended*: the plan said "in the transcript", and rendering a transcript is a client's job (plan 09); what this plan makes true is the shape in the tree, asserted in `subagent::runs_on_a_branch`.
- Every routing decision is audited with its signal values. `Router` holds an `Audit`, not an `Option<Audit>`, so the record is unconditional and a router built without a sink writes to the null one.

## Open questions

1. **Should the router ever be a model call?** §4.6 left this open and then settled it: the `router` role can be bound to an agent, so a model router is available without a new mechanism. Unbound stays the default because eval comparison needs reproducibility. Confirm no code path assumes the router is always declarative.

   **Decided: unbound by default, bindable, and nothing assumes otherwise.** `router` is an ordinary `Role` that `Bindings::resolve` binds like any other, and `Bindings::is_declarative(Role::Router)` is the **one** place the question is asked — there is no `if role == Router` anywhere else, and `roles::router_is_unbound_by_default` asserts both halves. `orrery-ext-agents-default` deliberately ships no router agent, so the floor is declarative and reproducible; binding one is a `[roles]` line. `Router::decide` stays sync and pure either way: a bound router agent is run by the **orchestrator** as an ordinary step, and its answer arrives as a `Proposal` that `decide` judges against the rules exactly as it judges the main model's. A model router therefore cannot escape the rules or the ladder, which is the whole reason it is the same mechanism.

2. **`child_cost` calibration.** A declared value that is wrong produces bad fan-out. Should the telemetry stream feed back a measured per-child cost the user can copy into config? Cheap and useful; suggest a `orrery telemetry suggest` later.

   **Decided: the router reads a declared value only, and the feedback loop is a later, separate command.** `FanOutProfile::child_cost` is `Option<u64>`, and nothing here measures, estimates or remembers anything — `decide` is pure, and a fan-out width that depended on what happened last time would not be reproducible across eval runs. A wrong value is bounded rather than dangerous: it is one of three `min` terms, so over-declaring costs children, under-declaring is caught by the cap and by the whole-workflow ceiling, and `fanout::no_child_cost_still_bounded` asserts that declaring nothing at all is still safe. The measured number belongs in the telemetry stream, which already carries `model.request` token counts; `orrery telemetry suggest` reads that stream and prints a line to paste into config. **It does not write config and it does not call back into the router**, so it is deferred to the CLI plan with no hook needed here.

3. **Quorum semantics.** `Quorum(n)` — does the workflow proceed with n results and cancel the rest, or wait for all and use n? Cancelling is cheaper and probably intended. Decide and document.

   **Decided: proceed with `n` and cancel the rest.** Waiting for every branch and then using `n` pays for all of them and uses some — the expensive reading of a feature whose entire point is stopping early. `Join::wanted` is the number of answers needed, the runner drops the remaining futures the moment it has them (dropping a future *is* the cancellation), and the join's value records `cancelled` so the count is visible afterwards. `First` is `Quorum(1)`, spelled separately because that is what people write. A quorum larger than the number of branches is clamped rather than left waiting for an answer that cannot arrive. Asserted in `workflow::parallel_join_all_first_quorum`, all three cases.

4. **Nested workflows.** Can a workflow step be another workflow? Budgets compose awkwardly. Suggest: no, in phase 6; revisit with a real need.

   **Decided: no, in phase 6 — and enforced by the type rather than by a convention.** `Step` has no `Workflow` variant, so there is nowhere to write one. The reason is the budget: a whole-workflow ceiling is what makes a workflow's cost knowable before it runs, and a nested workflow carrying its own declared ceiling would either shadow the parent's (two ceilings, and the outer one no longer bounds anything) or be silently ignored (a declared value that does nothing, which is worse). What is available instead composes properly: `Parallel` nests, `Loop` nests, and both take their budget as a **slice of what is left** rather than a ceiling of their own. Revisit when there is a real case a sub-agent with a narrower grant cannot already express.

## State

**Done**, 2026-09-18, on `feat/harness_claude-0917`. 62 tests and 2 doctests across the three crates, all passing, and `cargo run -p xtask -- deps-check` clean.

- `orrery-router` — `Signals` (`Copy`, so nothing that needs fetching or generating can be added to it), `[[route]]` rules, `decide` with its three-level precedence and its one-rung-at-a-time ladder, the fan-out arithmetic, modes as `mode(...)` capability requests the policy engine answers, and role binding with one winner per role and the losers named.
- `orrery-orchestrator` — the step types, `Expr`/`Predicate` evaluation, the load-time dataflow typecheck (translation #5) behind a `Checked` newtype the runner demands, the whole-workflow budget, the join semantics, the workflow machine and sub-agents on branches.
- `orrery-ext-agents-default` — the five shipped role agents, loading through the same door a third-party bundle uses.

**Not built here, and why.** Two joins to the rest of the system are declared as traits and left for the facade, because implementing them here would be the kernel dependency this plan exists to avoid: `StepExecutor` (run a sub-agent, call a tool) and `TurnRunner` (run a child's turns under a lease). Wiring them to `orrery-kernel` and `orrery-tools` belongs to `orrery-harness`, the one crate `deps-check` lets link both halves. Until that lands the machine is complete and tested but not yet reachable from a live session, and no test in these three crates touches a provider, a model or a network.
