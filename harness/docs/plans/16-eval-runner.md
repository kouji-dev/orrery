# 16 · Evals — reproducible numbers, in the core

**Goal.** Running a benchmark is a core capability, not an extension, because a reproducible run needs what only the kernel has: deterministic session construction, a pinned profile, budget enforcement, real token counts at the provider boundary, and an audit trail. An extension can add a suite; it cannot make a run reproducible. When this is done, one suite runs against two profiles and one competing harness with the same graders and comparable cost numbers.

**Covers.** §4.14 in full.

**State (2026-09-19).** Tasks 1-9 implemented and green: `cargo test -p orrery-grader
-p orrery-eval -p orrery-ext-graders` is 63 tests, `cargo clippy` is clean at
`-D warnings`, `xtask deps-check` prints ok. Every open question below is settled.
The one thing left is the CLI dispatch in `orrery-cli` - see **Done when**.
No test in this plan calls a model, a network or an agent CLI: the providers replay
committed `.jsonl` fixtures and the competing harness is a fake binary the test writes.

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

Also pinned per run: the profile, the model, the extension version set (plan 15), the seed where a provider supports it, and wasm fuel (plan 14) for deterministic guest execution. **Plan 14 answered the how:** `consume_fuel` is a `Config` setting and `Config` is per-`Engine`, so the eval runner needs its **own** engine, not a per-run flag. `orrery_host_wasm::WasmHost::for_eval()` is it.

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

- [x] **Failing test first.** `grader::is_object_safe` and a doc test showing a minimal grader.
- [x] Implement. Keep it tiny — it is published and it should never need a breaking change.

`GradeInput` carries one field the sketch above does not: `config`, the case's own
`[case.grade]` block, which the runner passes through **unread**. Without it a grader
cannot be told what to check, and the only alternative — the runner parsing assertions on
the grader's behalf — is the coupling this crate exists to prevent. `Score` likewise
carries `judge_cost`, which is what makes task 5's "the judge's tokens appear separately"
expressible at all.

### Task 2 · Case and run types

Files: `orrery-eval/src/{case,run,matrix}.rs`

- [x] **Failing test first.** `matrix_expands` — 2 profiles × 2 models × 2 seeds = 8 runs, in a deterministic order. (This plan called it `matrix::expands`; it landed as `matrix_expands` in `orrery-eval/tests/run.rs`, which is where to look for it.)
- [x] `run::defaults_are_reproducible` — a deserialized `EvalRun` with no `memory`/`router` keys has `Off` and `Declared`.
- [x] Implement.

### Task 3 · Isolation

Files: `orrery-eval/src/isolate.rs`, `tests/isolate.rs`

- [x] **Failing test first.** `isolate::cases_never_share_state` — two cases that both write the same path; assert neither sees the other's file.
- [x] `isolate::worktree_is_cleaned_up` — including after a panic.
- [x] `isolate::concurrency_is_honoured`.
- [x] `isolate::a_case_directory_name_is_short` — **added 2026-09-19.** The case directory is the runner's own contribution to path length, and on Windows it is spent against `MAX_PATH`. It ran to about 110 characters (the case, profile and model names in full plus a 32-hex uuid), which is what put a 143-character workspace's `Cargo.toml` over the limit and made the grader's read come back as a policy denial. Names are clipped to 20 characters and the uuid to 8 hex digits; the case id is still the first thing the name says. The misreported denial itself is plan 07's, and fixed there.
- [x] `eval::a_244_character_workspace_opens_its_session_store` — **the MAX_PATH residual, closed 2026-09-19.** Shortening the case directory above moved the limit but did not remove it: at a 244-character workspace the *state file* `<workspace>\.orrery\sessions.db` reaches 263 characters and a driven `orrery eval run` exited **6** with "session store backend failed: unable to open database file" before a single case was graded. Honestly reported as the filesystem error it was, so not a regression of the denial above — but the database was fine and only the path form was wrong. Fixed in the store (plan 02: every SQLite connection, the per-session writer included, opens in extended-length `\?\` form) and pinned from the **binary** here. Reverting that store change makes this test fail with exactly `left: Some(6)`.
- [x] Implement worktree isolation (git worktree per case). Container isolation is a later task — leave the enum variant and a clear `Unsupported` error.

Cleanup is in `Drop` rather than at the end of the run, which is what makes it survive a
panic; `isolate::worktree_is_cleaned_up_after_a_panic` panics with a workspace open and
then looks for the directory. `WorkspaceSpec::Fixture` (copy a directory) and `Empty` need
no git, so a suite that is not over a repository still isolates.

### Task 4 · Running and telemetry

Files: `orrery-eval/src/{run,report}.rs`, `tests/run.rs`

- [x] **Failing test first, and it is the one that makes the whole plan worth doing.** `run::cost_comes_from_telemetry` — run a case against the fixture provider with known token counts; assert `EvalResult.cost` matches exactly and is **not** a post-hoc estimate. Instrument to prove no estimation path exists.
- [x] `run::by_role_attribution` — a run with a cheap compactor and an expensive planner attributes tokens to the right roles.
- [x] `run::budget_exceeded_is_an_outcome` — not an error.
- [x] `run::transcript_is_replayable`.
- [x] Implement.

The proof is structural first and instrumented second. `BoundaryMeter` has **no method
that takes a `Usage`**: the only way in is `observe(&ModelEvent)`, which ignores every
variant but `Usage`, so there is no estimation path to audit because there is none to
write. On top of that, `HarnessRunner` wires the provider's own `TokenCounter` behind an
`EstimatorProbe`, and the test asserts the probe was asked **nothing** for a whole run.
(The session-side `CharsOverFour` fits the context to the window; it prices nothing, and
no cost reads it.)

Open question 4 is answered and has a test: `run::by_role_separates_even_on_one_model`
binds every role to one provider and the three lines stay separate, because attribution is
by pass, not by model.

### Task 5 · Graders

Files: `orrery-ext-graders/src/*`

- [x] **Failing test first.** `command::exit_code_is_the_score`; `command::runs_under_a_grant` — a grader script is brokered like anything else.
- [x] `assertion::checks_files_diffs_and_tool_calls`.
- [x] `model::counts_its_own_cost` — the judge's tokens appear in the result, separately from the run's.
- [x] Implement all three as one extension.

Both file and spawn access go through `BrokerFacade`, so a denied grader is
`GradeError::Unavailable` — recorded as `error`, never as the case failing. The same rule
covers a tool-call assertion with no session store bound: a safety check that could not be
evaluated must not look like one that held.

### Task 6 · Compare and replay

Files: `orrery-eval/src/{compare,replay}.rs`, `tests/compare.rs`

- [x] **Failing test first.** `compare::refuses_incomparable_runs` — two runs with different `memory` settings do not compare without `--force`, and the message says why.
- [x] `compare::reports_regressions`.
- [x] `replay::opens_the_failing_session` — replay a specific case and assert the transcript matches what the run recorded.
- [x] Implement.

### Task 7 · Cross-harness adapters

Files: `orrery-eval/src/adapter.rs`, `tests/adapter.rs`

- [x] **Failing test first, and it is the phase-9 criterion.** `adapter::one_suite_two_profiles_one_competitor` — run a small suite against two of our profiles and one external CLI (stubbed by a fake binary in tests), same graders; assert comparable outcomes and that the external run's cost is **labelled as reported-by-the-tool**.
- [x] `adapter::argv_reuses_ade_knowledge` — cite and port the argv/shim handling from `ade/src-tauri/src/agents/adapters/`.
- [x] Implement.

The competitor in the test is a fake binary the test writes — a `.cmd` on Windows, a `sh`
script elsewhere — so **no real agent CLI is started and no key is needed**. On Windows
that fake is itself the exercise: a `.cmd` cannot be spawned directly, so the ported
`launch_prefix` is what makes it start at all. Ported one for one: extension order with
the bare name **last**, `.cmd`/`.bat` through `cmd.exe /c call`, `.ps1` through `pwsh`
then `powershell.exe`, and the extensionless-to-sibling redirect for os error 193. Not
ported: the probe cache, the registry PATH sweep and the hook merging, none of which a
one-shot eval run needs.

### Task 8 · Singleton conformance as a suite

Files: `orrery-eval/src/lib.rs`

- [x] Wire plans 02, 07 and 12's conformance suites so `orrery eval run conformance` runs them against the currently bound singletons.
- [x] **Failing test first.** `conformance::detects_a_widening_permission_handler` — plug in a deliberately broken handler; the suite fails with a clear message.

Plans 02 and 12 own their suites and are called as they are. Plan 07 has no
`run_conformance`, so the handler check is written here, in
`conformance::check_permission_handler` — and here is the right place for it: plan 07
already stops a widening handler *mechanically* inside `review_narrowing`, so what was
missing was not enforcement but **visibility**. Note that a handler outside
`orrery-policy` cannot return `Allow` at all, because minting a token is private to that
crate; the widening it can attempt is `Deny -> Ask`, and that is what the test plugs in.
An unbound singleton is skipped, not failed.

### Task 9 · CI shape

Files: `orrery-eval/src/junit.rs`

- [x] **Failing test first.** `junit::exits_non_zero_on_regression` against a baseline file.
- [x] Implement JUnit output.

---

## Done when

- [x] `cargo test -p orrery-grader -p orrery-eval -p orrery-ext-graders` green. 63 tests.
- [x] One suite runs against two profiles and one competing harness with the same graders —
  `adapter::one_suite_two_profiles_one_competitor`, the competitor stubbed by a fake binary
  because nothing here may call a real agent CLI. **And from the binary, 2026-09-19:** the
  library half above was green while nothing a suite could *say* reached it — `run.rs`,
  `case.rs` and `matrix.rs` between them mentioned `adapter` once, in a doc comment. A
  suite now declares its competitor in `[adapter.<id>]`, `Matrix::expand_with_adapters`
  grows the point, `EvalRunner::with_adapters` binds it, and `orrery eval run` puts both
  harnesses in one report:

  ```text
  Fail  writes-a-file·careful/profile-default  1 turns, 0 tool calls  0.00  [823 tokens, measured at the provider boundary]
  Pass  writes-a-file·fake-agent/profile-default  1 turns, 0 tool calls  1.00  [1540 tokens, reported by fake-agent]
  Fail  writes-a-file·fast/profile-default  1 turns, 0 tool calls  0.00  [823 tokens, measured at the provider boundary]
  ```

  The competitor is a fake `.cmd` in a temporary directory in the tests too — `eval.rs`'s
  `a_suite_puts_a_competing_harness_beside_ours`, `the_text_report_labels_whose_number_each_one_is`
  and `two_cross_runs_compare` — because **no real agent CLI is started and no key exists**.
  A bare `[adapter.claude]` takes its argv from `known_adapter`, the ADE's own; an id with
  no known argv and no `command` is refused by name, and so is a `command` whose program is
  not on `PATH` — both before the first case runs, not an hour into a suite.
- [x] Cost numbers are read at the provider boundary, provably: no API accepts a `Usage`,
  and the estimator probe is asked nothing for a whole run.
- [x] A failed case opens with `replay::replay` — **as a library call.**
- [x] `--format junit` exits non-zero on regression — `junit::exit_code`, **as a library
  call.**

~~**Not done: the CLI wiring.**~~ **Done, 2026-09-19, and the estimate was about
right.** `orrery eval run|compare|replay` is something a person can type, and
`orrery-cli/tests/eval.rs` drives the **binary** — a suite file in, an isolated fixture
workspace, the assertion grader, a report on stdout, and a red suite exiting non-zero.
Three decisions had to be made that a library cannot make for itself, and they are
written down in `cmd/eval.rs`:

- **A run id has to name something.** Reports are written to
  `<state-dir>/eval/<run-id>.json`, which is what gives `compare` and `replay` arguments
  that mean anything. Both also take a path, so a person holding the file does not have
  to learn where the harness filed it.
- **One store, many workspaces.** The cases share one session database under the state
  directory so `eval replay` can re-open a transcript afterwards; the *workspaces* stay
  one per case and still vanish on drop. `Isolator` is untouched.
- **Every matrix point is bound to the same fixture runner**, because the fixture
  provider is the only selectable one. Said out loud in the source, because a matrix
  whose points are secretly identical reports agreement it did not measure. When a real
  provider becomes selectable (plan 10), that binding is the only line that changes.
  **Amended 2026-09-19:** every point *of ours*. A point whose profile is one of the
  suite's `[adapter.<id>]` blocks is skipped here and bound by `with_adapters` instead,
  because a competitor's run is not ours to point a provider at.

**Task 7 amended, 2026-09-19.** Its two boxes were ticked against library tests, and
`orrery eval run` could reach neither: the suite format had no way to name a competitor.
The files this adds to the task's list are `orrery-eval/src/{case,matrix,run}.rs` and
`orrery-cli/src/cmd/eval.rs`. What it deliberately does **not** add is a way to pick the
competitor's model: an adapter is one matrix point, not one per model and seed, because
imposing our model axis on another harness would put a label on their run that we did not
set.

The graders come through `orrery_harness::features::graders`, behind a new `graders`
feature that is **in the default set**: `orrery-ext-graders` is an extension, `orrery-cli`
is a core crate, and `deps-check` rule 1 excuses only the facade — so the facade is where
it goes. The judge grader is not installed: it costs money and this build cannot pay.

## Open questions

1. **Container isolation. Settled: worktree-only, and it says so.** `Isolation::Container`
   is in the enum and returns `EvalError::Unsupported` naming itself and this question,
   rather than quietly degrading to a worktree.
2. **Seeds. Settled: labelled, never claimed.** `Matrix::seeds` documents that a seed is
   real for a local model and a label for a hosted one, and `Reproducibility::seeded`
   records only that one was *asked for*. Nothing in the crate calls a seeded run
   reproducible.
3. **Baseline storage. Settled: a committed file.** `junit::Baseline` is JSON in the
   repository. A regression threshold that can change without a review is not a
   threshold.
4. **`by_role` with everything on one model. Settled: it separates.**
   `run::by_role_separates_even_on_one_model` binds all three roles to one provider, and
   the lines stay apart because the meter is told which role is taking the pass.

   One shape note from building it: `by_role` is a `ByRole` keyed on the role's wire tag
   rather than a `BTreeMap<Role, _>`. `orrery_proto::Role` derives no `Ord` and is
   `#[non_exhaustive]`, and adding a derive to a crate this plan does not own is not this
   plan's business.

---

## State

**Landed.** `orrery-eval` builds and runs eval suites, and the CLI reaches it:
`orrery eval` is implemented, not a `not_implemented` stub.

- The matrix expands profile-major, then model, then seed, and that order is the
  contract. The test is `matrix_expands` in `orrery-eval/tests/run.rs` — this
  plan's Task 2 originally named it `matrix::expands`, which is amended in place
  above.
- Reproducibility is a property of the run: an `EvalRun` that declares memory or
  a router is not pinned, and `run::defaults_are_reproducible` holds the
  defaults (`Off`, `Declared`).
- Every eval in this repository's suite runs against committed fixtures through
  the fixture provider. No eval reaches a network or a paid API, which is a
  precondition a test can hold rather than a convention.
