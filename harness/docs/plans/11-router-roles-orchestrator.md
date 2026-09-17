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

---

## File structure

**Create**

- `harness/core/crates/orrery-router/src/{lib,signals,rules,decide,fanout,mode}.rs`
- `harness/core/crates/orrery-router/tests/{decide,fanout}.rs`
- `harness/core/crates/orrery-orchestrator/src/{lib,step,workflow,subagent,expr,typecheck,budget,join}.rs`
- `harness/core/crates/orrery-orchestrator/tests/{workflow,subagent,loops,typecheck}.rs`
- `harness/extensions/crates/orrery-ext-agents-default/{Cargo.toml,orrery.toml,README.md,src/lib.rs}`

---

## Tasks

### Task 1 · Signals and declarative rules

Files: `orrery-router/src/{signals,rules}.rs`

- [ ] **Failing test first.** `rules::signals_are_cheap` — a compile-level check that `Signals` contains no handle, no future and nothing requiring a model call; plus a doc note listing what is deliberately absent.
- [ ] `rules::parse_from_config`.
- [ ] Implement.

### Task 2 · `decide`

Files: `orrery-router/src/decide.rs`, `tests/decide.rs`

- [ ] **Failing test first.** `decide::user_instruction_beats_a_rule` — explicit instruction wins over a declared rule that says otherwise.
- [ ] `decide::rule_beats_a_model_proposal` — a proposal to fan out where a rule forbids it is denied.
- [ ] `decide::rungs_are_climbed_one_at_a_time` — a proposal jumping from one pass to a workflow is downgraded unless explicitly instructed.
- [ ] `decide::is_pure` — same signals in, same decision out, 1000 times.
- [ ] `decide::is_audited_with_signal_values` — the audit entry contains the numbers that produced the decision.
- [ ] Implement.

### Task 3 · Fan-out arithmetic

Files: `orrery-router/src/fanout.rs`, `tests/fanout.rs`

- [ ] **Failing test first.** `fanout::n_is_the_minimum` (proptest) — for arbitrary units, caps and budgets, N never exceeds any of the three bounds.
- [ ] `fanout::overlapping_inputs_collapse_to_one` — inputs that are not disjoint produce N = 1.
- [ ] `fanout::no_child_cost_still_bounded` — with no declared `child_cost`, the cap and units still bound N.
- [ ] Implement.

### Task 4 · Modes as permissions

Files: `orrery-router/src/mode.rs`

- [ ] **Failing test first.** `mode::switch_is_a_policy_question` — switching to `execute` without `mode(execute)` is denied by the policy engine, not by the router.
- [ ] `mode::plan_is_read_only` — in `plan` mode, a write tool is not in `visible()`.
- [ ] Implement.

### Task 5 · `Expr` evaluation and the load-time typecheck

Files: `orrery-orchestrator/src/{expr,typecheck}.rs`, `tests/typecheck.rs`

- [ ] **Failing test first, and it is translation #5.** `typecheck::forward_ref_is_rejected_at_load` — step 1 referencing step 3 fails before anything runs, naming the file and step.
- [ ] `typecheck::path_must_match_returns` — a ref to `.count` on a step whose `returns` has no `count` fails at load.
- [ ] `typecheck::valid_workflow_passes`.
- [ ] `expr::cannot_call_out` — a review-level assertion plus a test that the evaluator has no I/O in its signature.
- [ ] Implement.

### Task 6 · Sub-agents on branches

Files: `orrery-orchestrator/src/subagent.rs`, `tests/subagent.rs`

- [ ] **Failing test first.** `subagent::grant_is_intersected` — a sub-agent declaring a wider grant than its parent gets the intersection; assert it cannot do what the parent could not.
- [ ] `subagent::budget_is_mandatory` — an `AgentDefinition` without a budget fails to deserialize.
- [ ] `subagent::runs_on_a_branch` — the sub-agent's turns are ordinary turns in the tree, inspectable, not collapsed into one tool result.
- [ ] `subagent::parent_merges_at_the_join` — cross-reference plan 02 task 8; assert no deadlock and a `BranchResult` row.
- [ ] `subagent::cannot_prompt_fails_typed` — a sub-agent that cannot prompt returns a typed error the parent handles, rather than auto-denying into a stall.
- [ ] Implement.

### Task 7 · Workflows and loops

Files: `orrery-orchestrator/src/{workflow,join}.rs`, `tests/{workflow,loops}.rs`

- [ ] **Failing test first, and it is the phase-6 criterion.** `loops::terminates_on_its_own_cap` — a verify loop whose gate never passes stops at `max_iterations` with a typed outcome, and the kernel enforced it rather than the prompt.
- [ ] `loops::predicate_is_mandatory` — a `Loop` step without `until` fails at load.
- [ ] `workflow::tool_step_makes_no_model_call` — assert the provider was never invoked.
- [ ] `workflow::parallel_join_all_first_quorum` — three cases.
- [ ] `workflow::gate_on_fail` — stop, retry and escalate.
- [ ] `workflow::budget_is_whole_workflow` — the ceiling applies across steps, not per step.
- [ ] Implement.

### Task 8 · Role binding

Files: `orrery-router/src/lib.rs`, `orrery-ext-agents-default/*`

- [ ] **Failing test first.** `roles::missing_binding_fails_at_session_start` — a profile binding `planner` to a nonexistent agent fails at startup, naming the file and line, not mid-turn.
- [ ] `roles::exactly_one_wins` — two extensions offering a planner; one is bound by precedence; the audit records which ran and the ledger names the loser.
- [ ] `roles::phases_fire_in_every_step` — an interceptor registered once observes passes under the planner *and* the executor.
- [ ] `roles::binding_never_widens`.
- [ ] Implement the default agents as a real extension with a manifest.

---

## Done when

- `cargo test -p orrery-router -p orrery-orchestrator -p orrery-ext-agents-default` green.
- A verify loop terminates on its own cap, enforced by the kernel.
- An invalid workflow fails at load, not after three model calls.
- A sub-agent's work is visible as ordinary turns in the transcript.
- Every routing decision is audited with its signal values.

## Open questions

1. **Should the router ever be a model call?** §4.6 left this open and then settled it: the `router` role can be bound to an agent, so a model router is available without a new mechanism. Unbound stays the default because eval comparison needs reproducibility. Confirm no code path assumes the router is always declarative.
2. **`child_cost` calibration.** A declared value that is wrong produces bad fan-out. Should the telemetry stream feed back a measured per-child cost the user can copy into config? Cheap and useful; suggest a `orrery telemetry suggest` later.
3. **Quorum semantics.** `Quorum(n)` — does the workflow proceed with n results and cancel the rest, or wait for all and use n? Cancelling is cheaper and probably intended. Decide and document.
4. **Nested workflows.** Can a workflow step be another workflow? Budgets compose awkwardly. Suggest: no, in phase 6; revisit with a real need.
