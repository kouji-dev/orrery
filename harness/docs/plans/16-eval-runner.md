# 16 · Evals — reproducible numbers, in the core

**Goal.** Running a benchmark is a core capability, not an extension, because a reproducible run needs what only the kernel has: deterministic session construction, a pinned profile, budget enforcement, real token counts at the provider boundary, and an audit trail. An extension can add a suite; it cannot make a run reproducible. When this is done, one suite runs against two profiles and one competing harness with the same graders and comparable cost numbers.

**Covers.** §4.14 in full.

**Crates.** `core/crates/orrery-grader` (published trait) · `core/crates/orrery-eval` · `extensions/crates/orrery-ext-graders`.

**Depends on.** [`02`](02-session-store.md) (replay), [`05`](05-kernel-loop.md), [`07`](07-policy-broker-audit.md) (telemetry), [`10`](10-config-layers.md) (profiles), [`11`](11-router-roles-orchestrator.md) (role bindings for `byRole` cost).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **Where the numbers come from:** the kernel's telemetry at the provider boundary, never estimated afterwards.
- **Pluggability threatens reproducibility, so a run pins it.** `memory: "off"` and `router: "declared"` are the defaults.
- Cases never share state: `isolation` is `worktree` or `container`.
- Graders are extensions; the trait is a separate published crate (translation #15) so a grader never depends on the runner.

---

## Architecture

### The types

```rust
pub struct EvalCase {
    pub id: String,
    pub workspace: WorkspaceSpec,     // repo + commit, or a fixture archive
    pub prompt: String,
    pub grade: GraderSpec,
    pub budget: Budget,
}

pub struct EvalRun {
    pub suite: String,
    pub matrix: Matrix,               // profile[] × model[] × seed[]
    pub concurrency: u32,
    pub isolation: Isolation,         // Worktree | Container
    pub memory: MemoryMode,           // Off (default) | Provider { id, scopes }
    pub router: RouterMode,           // Declared (default) | Agent { name }
}

pub struct EvalResult {
    pub case: String, pub profile: String, pub model: String,
    pub outcome: EvalOutcome,         // Pass | Fail | Error | BudgetExceeded
    pub score: Option<f64>,
    pub cost: Usage,
    pub by_role: BTreeMap<Role, RoleCost>,   // §4.6 bindings — where the tokens actually went
    pub timing: Timing,               // wall, model, tool
    pub turns: u32, pub tool_calls: u32,
    pub transcript: SessionRef,       // full, replayable
}
```

`by_role` is the number that makes §4.6's "plan with a large model, compact with a cheap one" checkable rather than asserted. It is also the reason the eval runner has to know about role bindings.

### Reproducibility is declared, not hoped for

A memory provider reading a mutable `global` store and a `router` bound to a model both make the same case behave differently on two days. So an `EvalRun` declares both, and the defaults are the reproducible ones. A run with `memory` on or `router` bound is still useful — it is just labelled as what it is, and `eval compare` refuses to compare across differing reproducibility settings without `--force`.

Also pinned per run: the profile, the model, the extension version set (plan 15), the seed where a provider supports it, and wasm fuel (plan 14) for deterministic guest execution.

### The singleton conformance suites live here

§4.14 makes a second, quieter promise: the same runner keeps the singleton contracts honest. A conformance suite feeds a `SessionStore` branch/compact/replay cases, checks a `MemoryProvider` against the scope lifetimes, and offers a `PermissionHandler` decisions it must not widen.

Those suites are written in plans 02, 12 and 07. This plan **runs them as an eval suite**, so "does this alternative session backend actually work" is one command.

### Graders

```rust
// orrery-grader — published, so a grader extension never depends on orrery-eval
#[async_trait]
pub trait Grader: Send + Sync {
    async fn grade(&self, input: GradeInput) -> Result<Score, GradeError>;
}
pub struct GradeInput { pub workspace: PathBuf, pub transcript: SessionRef, pub case: CaseRef }
pub struct Score { pub outcome: EvalOutcome, pub score: Option<f64>, pub detail: Surface }
```

| Grader | How it scores | Use for |
|---|---|---|
| `command` | Exit code of a script (tests, build, lint) | SWE-style patch benchmarks |
| `assertion` | Declared checks on files, diffs, tool calls | Behavioural and safety checks |
| `model` | A judge model with a rubric | Open-ended quality, **with its own cost counted** |

The model grader's cost counting is not a detail: a judge that costs more than the run it grades should be visible.

### Suites are just packages

Public suites and internal regression sets install from the registry like any extension, declare their fixtures, and run under the same permission grants. A team's private suite over their own monorepo is the common case and needs no special treatment.

### Cross-harness comparison

The runner takes an adapter, so Claude Code, Codex or Pi can be driven as an external agent under the same cases, budgets and graders.

```toml
[adapter.codex]
command = "codex exec --json"
parse   = "jsonl"

[adapter.claude-code]
command = "claude --print --output-format json"
```

**Reuse what the ADE already knows.** `ade/src-tauri/src/agents/adapters/{claude,codex,pi}.rs` already builds argv, resolves shims on Windows, parses model lists and merges hook config for exactly these CLIs. That is the hard-won part; the adapter here is a thin wrapper over that knowledge. Do not rediscover it.

Cost numbers from an external harness are necessarily weaker than ours (we read them at our provider boundary; for an adapter we read what the CLI reports). **Label them as such in the output** rather than presenting two incomparable numbers side by side as if they were the same measurement.

### CLI

```bash
orrery eval run swebench-lite --profile review,fast --model claude-sonnet-5,local/qwen
orrery eval compare run-812 run-819
orrery eval replay run-812 --case api-42     # opens the exact session that failed
orrery eval run --profile ci --format junit  # exits non-zero on regression against a baseline
```

`replay` is what makes a failed case actionable, and it works because the turn tree is canonical and immutable (plan 02).

---

## File structure

**Create**

- `harness/core/crates/orrery-grader/src/lib.rs`
- `harness/core/crates/orrery-eval/src/{lib,case,run,matrix,isolate,report,compare,replay,adapter,junit}.rs`
- `harness/core/crates/orrery-eval/tests/{run,isolate,compare,adapter}.rs`
- `harness/extensions/crates/orrery-ext-graders/{Cargo.toml,orrery.toml,README.md,src/{command,assertion,model}.rs}`

---

## Tasks

### Task 1 · The grader trait

Files: `orrery-grader/src/lib.rs`

- [ ] **Failing test first.** `grader::is_object_safe` and a doc test showing a minimal grader.
- [ ] Implement. Keep it tiny — it is published and it should never need a breaking change.

### Task 2 · Case and run types

Files: `orrery-eval/src/{case,run,matrix}.rs`

- [ ] **Failing test first.** `matrix::expands` — 2 profiles × 2 models × 2 seeds = 8 runs, in a deterministic order.
- [ ] `run::defaults_are_reproducible` — a deserialized `EvalRun` with no `memory`/`router` keys has `Off` and `Declared`.
- [ ] Implement.

### Task 3 · Isolation

Files: `orrery-eval/src/isolate.rs`, `tests/isolate.rs`

- [ ] **Failing test first.** `isolate::cases_never_share_state` — two cases that both write the same path; assert neither sees the other's file.
- [ ] `isolate::worktree_is_cleaned_up` — including after a panic.
- [ ] `isolate::concurrency_is_honoured`.
- [ ] Implement worktree isolation (git worktree per case). Container isolation is a later task — leave the enum variant and a clear `Unsupported` error.

### Task 4 · Running and telemetry

Files: `orrery-eval/src/{run,report}.rs`, `tests/run.rs`

- [ ] **Failing test first, and it is the one that makes the whole plan worth doing.** `run::cost_comes_from_telemetry` — run a case against the fixture provider with known token counts; assert `EvalResult.cost` matches exactly and is **not** a post-hoc estimate. Instrument to prove no estimation path exists.
- [ ] `run::by_role_attribution` — a run with a cheap compactor and an expensive planner attributes tokens to the right roles.
- [ ] `run::budget_exceeded_is_an_outcome` — not an error.
- [ ] `run::transcript_is_replayable`.
- [ ] Implement.

### Task 5 · Graders

Files: `orrery-ext-graders/src/*`

- [ ] **Failing test first.** `command::exit_code_is_the_score`; `command::runs_under_a_grant` — a grader script is brokered like anything else.
- [ ] `assertion::checks_files_diffs_and_tool_calls`.
- [ ] `model::counts_its_own_cost` — the judge's tokens appear in the result, separately from the run's.
- [ ] Implement all three as one extension.

### Task 6 · Compare and replay

Files: `orrery-eval/src/{compare,replay}.rs`, `tests/compare.rs`

- [ ] **Failing test first.** `compare::refuses_incomparable_runs` — two runs with different `memory` settings do not compare without `--force`, and the message says why.
- [ ] `compare::reports_regressions`.
- [ ] `replay::opens_the_failing_session` — replay a specific case and assert the transcript matches what the run recorded.
- [ ] Implement.

### Task 7 · Cross-harness adapters

Files: `orrery-eval/src/adapter.rs`, `tests/adapter.rs`

- [ ] **Failing test first, and it is the phase-9 criterion.** `adapter::one_suite_two_profiles_one_competitor` — run a small suite against two of our profiles and one external CLI (stubbed by a fake binary in tests), same graders; assert comparable outcomes and that the external run's cost is **labelled as reported-by-the-tool**.
- [ ] `adapter::argv_reuses_ade_knowledge` — cite and port the argv/shim handling from `ade/src-tauri/src/agents/adapters/`.
- [ ] Implement.

### Task 8 · Singleton conformance as a suite

Files: `orrery-eval/src/lib.rs`

- [ ] Wire plans 02, 07 and 12's conformance suites so `orrery eval run conformance` runs them against the currently bound singletons.
- [ ] **Failing test first.** `conformance::detects_a_widening_permission_handler` — plug in a deliberately broken handler; the suite fails with a clear message.

### Task 9 · CI shape

Files: `orrery-eval/src/junit.rs`

- [ ] **Failing test first.** `junit::exits_non_zero_on_regression` against a baseline file.
- [ ] Implement JUnit output.

---

## Done when

- `cargo test -p orrery-grader -p orrery-eval -p orrery-ext-graders` green.
- One suite runs against two profiles and one competing harness with the same graders.
- Cost numbers are read at the provider boundary, provably.
- A failed case opens with `eval replay`.
- `--format junit` exits non-zero on regression.

## Open questions

1. **Container isolation.** Worktrees are enough for a monorepo suite and not enough for SWE-bench-style cases that install dependencies. Docker is the obvious answer and a heavy one. Phase 9 can ship worktree-only; say so.
2. **Seeds.** Most providers do not offer deterministic sampling, so `seed` in the matrix is aspirational for hosted models and real for local ones. Do not imply more determinism than exists — label runs accordingly.
3. **Baseline storage.** Where does the regression baseline live — a committed file, or the registry? Committed file is simpler and reviewable. Probably that.
4. **Is `by_role` cost attribution accurate when a role is bound to the same model as the main loop?** It should be, since attribution is by step not by model, but verify with a test that binds everything to one model and checks the numbers still separate.
