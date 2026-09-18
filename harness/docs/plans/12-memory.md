# 12 · Memory — scopes the kernel owns, stores it does not

**Goal.** The only component we deliberately do not implement. The kernel owns scoping, lifetime and visibility; an extension owns the store, the content and the retrieval strategy. Reads enter at `context.build`, writes only from lifecycle handlers, and a sub-agent's notes die with its branch without anyone writing cleanup code.

**Covers.** §4.3 in full.

**Crates.** `core/crates/orrery-memory` (published trait + rules + conformance) · `extensions/crates/orrery-ext-memory-file` (the reference provider).

**Depends on.** [`01`](01-proto-shared-types.md), [`05`](05-kernel-loop.md) (`context.build` and lifecycle points), [`07`](07-policy-broker-audit.md) (`mem.read` / `mem.write` aspects).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **Interceptors do no I/O**, so memory never writes from inside the loop. It writes from **lifecycle handlers**, which may reach the broker and return nothing, so they cannot change the turn they fire on.
- Recalled content is recorded in the turn as **resolved content, never a pointer**.
- Assembly order is fixed: stable prefix first, memory in the volatile suffix.
- The token clamp is ours; the store is theirs.

---

## Architecture

### Scopes are handles on things the kernel already creates and destroys

Never a free-form label, so lifetime and cleanup come for free.

| Scope | Lives as long as | Cleared at |
|---|---|---|
| `global` | the install | an explicit `forget` |
| `workspace` · `project` | the config layer it sits in | the folder leaves the config set |
| `session` | one session | `session.end` |
| `workflow` | one declared run | the workflow's budget closing |
| `branch` | a sub-agent or a retry branch | the branch, discarded ones included |
| `turn` | one user turn | `turn.end` |

**A sub-agent's notes dying with its branch is the case that earns the design**: nobody writes cleanup code, and an abandoned retry leaves no residue.

### Two doors, and the split is not cosmetic

```rust
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    fn id(&self) -> &str;
    /// Contributes to context.build, alongside skills, under a profile allowance.
    async fn recall(&self, q: RecallQuery) -> Result<Vec<MemEntry>, MemError>;
    /// Lifecycle handlers only. Enforced by the type of the context that can call it.
    async fn write(&self, scope: MemScope, entry: MemEntry) -> Result<(), MemError>;
    async fn forget(&self, scope: MemScope, sel: MemSelector) -> Result<u64, MemError>;
}

pub struct RecallQuery { pub scopes: Vec<MemScope>, pub query: String, pub budget: TokenBudget }
```

Zero or many providers can be active. `recall` runs during `context.build` under a **token allowance set by the profile**, and what comes back is **clipped to it**: memory shares the window with history, and a chatty provider must not quietly evict the transcript.

Enforcing "writes only from lifecycle handlers" in the type system: `write` and `forget` take a `&LifecycleCtx` witness parameter that only a lifecycle handler holds. An interceptor has an `InterceptCtx`, which cannot produce one.

```rust
async fn write(&self, _w: &LifecycleWitness, scope: MemScope, entry: MemEntry) -> Result<(), MemError>;
```

### Two rules that are not negotiable

1. **Visibility copies §4.10's sub-agent rule.** Read down your own chain, never across siblings, never write to a scope wider than the one you run in — otherwise a sub-agent denied `write` puts a secret in `global` for its parent to read back.
2. **Whatever memory injected is recorded in the turn as resolved content**, never as a pointer. The store moves on; the tree still has to show what the model saw. That is `TurnKind::Recalled` from plan 02.

### Assembly order, and the reason is cost

Stable prefix first — system prompt, agent, tool descriptors, skills — then the volatile suffix: recalled memory, history, current input. Providers that advertise `cache` key on the prefix, so anything changing per pass sits behind everything that does not. **A memory provider injecting near the top would invalidate the cache every pass.**

### Permissions

`mem.read` and `mem.write` are **per-scope capabilities** (scoped by `MemScope`, not by path), so an organisation can forbid `global` writes outright while leaving `session` alone:

```toml
[permissions."agent:critic"]
deny = ["mem.write(global)", "mem.write(workspace)"]
```

### The reference provider — the doc's open question, answered

§4.3 and §8 left one thing unsettled: whether we ship a reference provider or memory is simply absent until one is installed.

**Ship it, as an optional feature, off by default.** `orrery-ext-memory-file`: file-backed, `global` and `session` only, JSONL under the state directory, naive substring-plus-recency retrieval. Reasons: the conformance suite needs a real implementation to be meaningful; "memory is absent" is a bad default for a first-run experience; and shipping one makes the scope rules concrete for anyone writing a better one. Reasons it stays off by default: it is not good, and a mediocre default memory is worse than none for evals, which is why `EvalRun.memory` defaults to `"off"` (plan 16).

---

## File structure

**Create**

- `harness/core/crates/orrery-memory/src/{lib,provider,scope,visibility,clamp,witness,error}.rs`, plus `perm.rs` (the `MemPermissions` trait) and `ledger.rs` (`MemEvent`, which the host forwards to the audit stream).
- ~~`harness/core/crates/orrery-memory/tests/conformance.rs`~~ — **amended: `src/conformance.rs` and `src/testing.rs`.** A `tests/` binary cannot be linked by another crate, and task 7's `file::passes_conformance` has to run exactly this suite. The same move `orrery-session` made, for the same reason. A two-line `tests/conformance.rs` still exists: it runs the suite against `testing::InMemoryProvider`.
- `harness/extensions/crates/orrery-ext-memory-file/{Cargo.toml,orrery.toml,README.md,src/*}`

---

## Tasks

### Task 1 · Scopes and lifetimes

Files: `src/scope.rs`

- [x] **Failing test first.** `scope::branch_scope_dies_with_the_branch` — write at `branch`, discard the branch, assert the entry is gone **without any explicit cleanup call**.
- [x] `scope::turn_scope_dies_at_turn_end`.
- [x] `scope::global_survives_a_session`.
- [x] Implement scope resolution bound to kernel object lifetimes, driven by lifecycle points.
  - Bound by **`Drop`**, not by a lifecycle callback: `Lifetimes::enter(scope)` hands back a `ScopeGuard`, and dropping it *is* discarding the branch, the turn or the session. `branch_scope_dies_with_the_branch` therefore contains no cleanup call of any kind — the only thing it does is `drop(guard)`. A lifecycle handler can still `retire()` explicitly, which is how `workspace`/`project` clear.
- [x] `scope::a_retired_scope_clears_lazily` — added, because open question 2 deserved a test rather than a sentence.

### Task 2 · The witness

Files: `src/witness.rs`

- [x] **Failing test first (compile-fail).** `witness::interceptor_cannot_write` — ~~`trybuild`~~ proving an interceptor context cannot produce a `LifecycleWitness`. This is the structural guarantee; prove it mechanically.
  - **Amended: two `compile_fail` doctests, not `trybuild`.** `trybuild` is in neither the workspace pins nor `Cargo.lock`, and adding it would mean a registry fetch this phase forbids. A `compile_fail` doctest is compiled against the crate **as an external dependency**, which is exactly the vantage point the guarantee needs, and it is what the rest of this repo already does (`orrery-policy`'s `CapabilityToken`, `orrery-session`'s `BranchLease`, `orrery-kernel`'s phase table). The two cases are at the top of `src/witness.rs`: an `InterceptCtx` has no `witness()`, and `LifecycleWitness`'s field is private so one cannot be built by hand either. A third, passing doctest shows the positive case, and `tests/witness.rs` holds the run-time half.
- [x] Implement.

### Task 3 · Recall and the clamp

Files: `src/clamp.rs`

- [x] **Failing test first.** `clamp::chatty_provider_is_cut` — a provider returning 100k tokens under a 2k allowance contributes 2k and the rest is dropped, with the drop recorded. The record is a `MemEvent::Clipped` in the kernel's ledger, which the host writes to the audit stream.
- [x] `clamp::history_is_not_evicted` — with memory and history competing, history keeps its share. `split_window` takes memory's share off the window **before** the provider is asked, and the two halves always add back up to `window.available()`, so history's share is not something a provider can reach at all.
- [x] `clamp::order_is_prefix_then_memory` — assert the assembled context puts recalled entries after tool descriptors (cross-reference plan 05 task 3). Asserted against plan 05's real `ContextDraft`, through a dev-dependency on `orrery-kernel`, rather than against a copy of its field order.
- [x] Implement. An entry that does not fit is dropped **whole**: half a recalled note is worse than none, because the model cannot tell it was truncated.

### Task 4 · Visibility

Files: `src/visibility.rs`

- [x] **Failing test first, and it is the security-relevant one.** `visibility::no_sibling_reads` — two sub-agent branches; one writes at `branch`, the other cannot read it.
- [x] `visibility::no_writing_wider_than_you_run` — a sub-agent at `branch` scope cannot write `global`, even with a provider that would allow it. Assert at the kernel, not the provider. The test's provider is `testing::InMemoryProvider`, which enforces nothing at all; the assertion is that it holds **zero** entries afterwards, so the refusal happened before the store was reached.
- [x] `visibility::reads_down_your_own_chain` — a child can read its parent's `session` scope, and its parent's `branch` scope too.
- [x] Implement. Width is a property of `Actor::run_scope`, and `Actor::sub_agent` pins it to `branch`: the constructor is what makes rule 3 structural rather than a check somebody can forget to call.

### Task 5 · Recorded as resolved content

Files: `src/lib.rs`, integration with plan 02

- [x] **Failing test first.** `record::recalled_is_in_the_turn` — after a turn where memory contributed, the stored turn contains a `Recalled` entry with the **content**, and replaying the session reproduces the same context even if the provider now returns something different. Swap the provider between the two halves of the test to prove it. Done: the second half builds a whole new `MemoryKernel` over a different provider, writes a contradicting entry under the same key, asserts that a fresh recall really does say something else, then replays the original rows through `orrery_session::materialise` and gets the original context back.
- [x] Implement. `Recall::to_turn_kind()` is the one door, and `orrery_memory::recalled_message` renders identically to `orrery-session`'s own rendering of a `Recalled` row — which is what makes a replay reproduce the context rather than something that merely resembles it.

### Task 6 · The conformance suite

Files: `tests/conformance.rs`

- [x] Write `run_conformance` covering every scope lifetime, the visibility rules, the clamp, and `forget` semantics. This is what §4.14 means by "checks a `MemoryProvider` against the scope lifetimes". Eight cases, each `pub` so a provider can run one on its own while it is being built, and each cleaning up after itself so a file-backed provider can run the suite against a real state directory without leaving anything behind.
- [x] An in-test memory provider so the suite is exercised before the file provider exists — `testing::InMemoryProvider`, deliberately **permissive**: it enforces nothing but the scopes it declared, so every visibility assertion in this crate is made against a store that would happily have done the wrong thing.

### Task 7 · The file provider

Files: `orrery-ext-memory-file/*`

- [x] **Failing test first.** `file::passes_conformance`.
- [x] `file::only_declares_global_and_session` — asking it for `branch` scope is a declared unsupported-scope error, not a silent success. For `write` **and** for `forget`: a `forget` that reported zero would be the same silent lie.
- [x] Implement: JSONL under the state dir, substring + recency retrieval, `forget` by selector.
- [x] Manifest: `runtime = "native"`, `[provides] memory = true`, `[requires] read/write = ["$STATE/memory/**"]`. Already correct in the scaffold, with `memory = "file"` rather than `true` because the field is a singleton naming an implementation.
- [x] **Off by default**: ~~a cargo feature~~, and not in `orrery-harness`'s `default`. **Amended, and half done.** The crate is off by default in the sense that matters today — nothing in `orrery-harness` names it, in `default` or anywhere else, so no build links it. The `memory-file` feature on the facade is **not wired**, because this wave forbids touching `orrery-harness`, which is another agent's crate. `[features] default = []` is declared here with a comment saying where the switch belongs. Follow-up: add `memory-file = ["dep:orrery-ext-memory-file"]` to `orrery-harness`, outside its `default` set.

### Task 8 · Permissions integration

Files: `src/lib.rs`

- [x] **Failing test first.** `perm::mem_write_global_can_be_denied` — with `deny = ["mem.write(global)"]`, a provider's global write is refused and audited; session writes still work. Driven through the real `PolicyEngine`, with the rule text exactly as the Permissions section above writes it.
- [x] Implement the `mem.read`/`mem.write` checks at the kernel boundary.
  - **`MemPermissions` is a trait here, not a dependency on `orrery-policy`.** `orrery-memory` has to be `publish = true`, because `orrery-ext-memory-file` depends on it and `deps-check` rule 2 says an extension reaches core only through published crates; `orrery-policy` and `orrery-audit` are both `publish = false`. So the engine is wired in by the host — and by this crate's own tests, as a dev-dependency. The same reasoning produced `ledger::MemEvent` instead of an `orrery-audit` dependency: the host drains it and writes each entry as an `AuditEvent`.
  - A rule's target is the scope's **kind** — `mem.write(global)` — not the handle, because a rule cannot name a uuid that did not exist when it was written.

---

## Done when

- `cargo test -p orrery-memory -p orrery-ext-memory-file` green, including both compile-fail tests. **True** — 20 test-binary cases across eight binaries, plus 4 doctests, two of which are the compile-fails.
- A sub-agent's branch-scoped notes vanish with the branch, with no cleanup code anywhere. **True** — `scope::branch_scope_dies_with_the_branch` calls nothing but `drop(guard)`.
- A replayed session reproduces its context even after the memory store has changed. **True, and now at the kernel too.** `record::recalled_is_in_the_turn` proves the algebra with hand-built `TurnRow`s; `orrery-harness`'s `memory::a_turn_records_what_memory_contributed` proves the writing — a real turn through the real kernel against the real sqlite store, provider swapped between the halves, same context.
- `mem.write(global)` can be denied per subject. **True** — `perm::mem_write_global_can_be_denied`, through the real `PolicyEngine`.
- ~~The file provider is off behind an `orrery-harness` feature.~~ **Amended: half true.** No build links it, because nothing in `orrery-harness` names it at all; the named `memory-file` feature on the facade is not wired, because this wave forbids editing that crate. See task 7.

## Open questions

1. **Multiple providers and ordering.** Zero or many are allowed. When three providers all return entries under one allowance, who gets the tokens? Suggest: proportional to declared priority, with a documented default of equal shares. Decide before more than one exists.

   **Decided: proportional to declared priority, which defaults to 1, so equal priorities are equal shares.** `MemoryProvider::priority()` defaults to `1` and `MemoryKernel::share_for` divides `allowance.available()` by the sum of priorities. Taken now rather than when a second provider appears, because the alternative — first-come-first-served out of one pool — makes a provider's contribution depend on registration order, which is a configuration detail nobody would think to look at when memory goes quiet. Each provider gets its own `Recall`, and therefore its own `TurnKind::Recalled` row, so a replay says which store said what.
2. **`workspace` / `project` scope clearing** — "the folder leaves the config set" is hard to observe. Is it cleared lazily on next resolve, or eagerly? Lazy is simpler and probably right; write it down.

   **Decided: lazy, and the laziness is split in two.** Retiring a scope is a fact about *resolution*: from that instant it stops appearing in `Actor::readable_scopes` and a write to it is refused, so nothing stale can reach a model. The bytes go later, on `MemoryKernel::sweep`, which is a lifecycle handler's job and may reach a store. That split is not a compromise, it is the only shape that works: the cheap half has to be synchronous, because it happens in `ScopeGuard::drop` where there is no runtime to await on, and the expensive half cannot be. `scope::a_retired_scope_clears_lazily` asserts both halves. A swept scope stays retired — it does not come back to life because nothing is left of it.
3. **Does `recall` deserve a cache?** It runs every pass and a naive provider will re-scan a file each time. The clamp bounds the output, not the work. Consider a kernel-side per-pass memo keyed by `(provider, scopes, query)`.

   **Decided: no kernel-side cache, and here is what would change that.** A memo keyed by `(provider, scopes, query)` is only sound if the kernel knows nothing was written in between — and writes come from lifecycle handlers, which the kernel does not sequence against a pass. The memo would therefore need an invalidation signal the kernel cannot derive: a correctness risk taken to avoid a cost nobody has measured. The work is also the provider's own. `orrery-ext-memory-file` re-reads its file and is fast because the file is small; a provider for which re-scanning is expensive is a provider that should hold an index, and it is the only component that knows when its index is stale. Revisit with a profile that puts `recall` on the critical path — the clamp already bounds what memory costs the *model*, which is the expensive half.
4. **Is substring+recency retrieval too weak to be worth shipping?** It is a reference implementation, not a recommendation. If it embarrasses us, ship it as `examples/` rather than `extensions/crates/`. Revisit after the first real user.

   **Decided: it stays in `extensions/crates/`, off by default.** Moving it to `examples/` would defeat both reasons it exists: the conformance suite needs a real `MemoryProvider` to be meaningful, and an example is not loaded through `orrery-host`, so it would not demonstrate the thing worth demonstrating — that a memory provider is an ordinary extension, with a ledger entry, a deny rule and `orrery ext test`. The embarrassment is answered by the default, not by the directory: nothing links it unless an embedder asks for it. Revisit after the first real user, as written.

---

## State

**Done, 2026-09-18.** All eight tasks implemented on `feat/harness_claude-0917`, in two
commits — `orrery-memory`, then `orrery-ext-memory-file`. `cargo test -p orrery-memory
-p orrery-ext-memory-file` is green: 20 cases across eight test binaries plus 4
doctests, two of which are the compile-fail proofs. `cargo clippy --all-targets -- -D
warnings` is clean on both crates and `cargo run -p xtask -- deps-check` prints `ok`.
Nothing in either crate, or in either test suite, touches a network or a model.

Four things diverge from the plan as written, each amended in place above: the
compile-fail proof is a pair of `compile_fail` doctests rather than `trybuild`; the
conformance suite and the in-test provider live in `src/` rather than `tests/`, so the
file provider can link them; `MemPermissions` and `ledger::MemEvent` are a trait and a
value in this crate rather than dependencies on `orrery-policy` and `orrery-audit`,
which are both `publish = false`; and the `memory-file` feature on `orrery-harness` is
not wired, because that crate belongs to another agent this wave.

**Amended 2026-09-18 — the kernel writes the row.** `orrery-kernel`'s
`turn.rs` carried `TODO(plan-12)`: nothing wrote a `TurnKind::Recalled` row, so
this plan's third Done-when held only inside this crate. It does now. The kernel
reads memory **once per turn**, at turn start, and appends one `Recalled` row per
contributing provider before the `User` row and inside the compaction floor;
`context.build` no longer reads memory at all, because a row in the branch plus
an injection would send the same content twice. `MemoryRecall` grew
`recall_rows`, which is what keeps one row per provider (open question 1's
answer, now load-bearing). `orrery-harness::KernelMemory` is the adapter, and the
facade is the one crate allowed to name both halves.

Two follow-ups remain, both small. Add `memory-file =
["dep:orrery-ext-memory-file"]` to `orrery-harness`, outside its `default` set,
and have the facade build the `MemoryKernel` **from resolved config** rather than
leaving an embedder to construct one: wire `PolicyEngine` into `MemPermissions`,
drain `MemoryKernel::ledger()` into the audit stream, and build the `Actor` from
the branch the turn runs on.
