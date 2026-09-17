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

- `harness/core/crates/orrery-memory/src/{lib,provider,scope,visibility,clamp,witness,error}.rs`
- `harness/core/crates/orrery-memory/tests/conformance.rs` — `pub fn run_conformance(p: Arc<dyn MemoryProvider>)`
- `harness/extensions/crates/orrery-ext-memory-file/{Cargo.toml,orrery.toml,README.md,src/*}`

---

## Tasks

### Task 1 · Scopes and lifetimes

Files: `src/scope.rs`

- [ ] **Failing test first.** `scope::branch_scope_dies_with_the_branch` — write at `branch`, discard the branch, assert the entry is gone **without any explicit cleanup call**.
- [ ] `scope::turn_scope_dies_at_turn_end`.
- [ ] `scope::global_survives_a_session`.
- [ ] Implement scope resolution bound to kernel object lifetimes, driven by lifecycle points.

### Task 2 · The witness

Files: `src/witness.rs`

- [ ] **Failing test first (compile-fail).** `witness::interceptor_cannot_write` — `trybuild` proving an interceptor context cannot produce a `LifecycleWitness`. This is the structural guarantee; prove it mechanically.
- [ ] Implement.

### Task 3 · Recall and the clamp

Files: `src/clamp.rs`

- [ ] **Failing test first.** `clamp::chatty_provider_is_cut` — a provider returning 100k tokens under a 2k allowance contributes 2k and the rest is dropped, with the drop recorded.
- [ ] `clamp::history_is_not_evicted` — with memory and history competing, history keeps its share.
- [ ] `clamp::order_is_prefix_then_memory` — assert the assembled context puts recalled entries after tool descriptors (cross-reference plan 05 task 3).
- [ ] Implement.

### Task 4 · Visibility

Files: `src/visibility.rs`

- [ ] **Failing test first, and it is the security-relevant one.** `visibility::no_sibling_reads` — two sub-agent branches; one writes at `branch`, the other cannot read it.
- [ ] `visibility::no_writing_wider_than_you_run` — a sub-agent at `branch` scope cannot write `global`, even with a provider that would allow it. Assert at the kernel, not the provider.
- [ ] `visibility::reads_down_your_own_chain` — a child can read its parent's `session` scope.
- [ ] Implement.

### Task 5 · Recorded as resolved content

Files: `src/lib.rs`, integration with plan 02

- [ ] **Failing test first.** `record::recalled_is_in_the_turn` — after a turn where memory contributed, the stored turn contains a `Recalled` entry with the **content**, and replaying the session reproduces the same context even if the provider now returns something different. Swap the provider between the two halves of the test to prove it.
- [ ] Implement.

### Task 6 · The conformance suite

Files: `tests/conformance.rs`

- [ ] Write `run_conformance` covering every scope lifetime, the visibility rules, the clamp, and `forget` semantics. This is what §4.14 means by "checks a `MemoryProvider` against the scope lifetimes".
- [ ] An in-test memory provider so the suite is exercised before the file provider exists.

### Task 7 · The file provider

Files: `orrery-ext-memory-file/*`

- [ ] **Failing test first.** `file::passes_conformance`.
- [ ] `file::only_declares_global_and_session` — asking it for `branch` scope is a declared unsupported-scope error, not a silent success.
- [ ] Implement: JSONL under the state dir, substring + recency retrieval, `forget` by selector.
- [ ] Manifest: `runtime = "native"`, `[provides] memory = true`, `[requires] read/write = ["$STATE/memory/**"]`.
- [ ] **Off by default**: a cargo feature, and not in `orrery-harness`'s `default`.

### Task 8 · Permissions integration

Files: `src/lib.rs`

- [ ] **Failing test first.** `perm::mem_write_global_can_be_denied` — with `deny = ["mem.write(global)"]`, a provider's global write is refused and audited; session writes still work.
- [ ] Implement the `mem.read`/`mem.write` checks at the kernel boundary.

---

## Done when

- `cargo test -p orrery-memory -p orrery-ext-memory-file` green, including both compile-fail tests.
- A sub-agent's branch-scoped notes vanish with the branch, with no cleanup code anywhere.
- A replayed session reproduces its context even after the memory store has changed.
- `mem.write(global)` can be denied per subject.

## Open questions

1. **Multiple providers and ordering.** Zero or many are allowed. When three providers all return entries under one allowance, who gets the tokens? Suggest: proportional to declared priority, with a documented default of equal shares. Decide before more than one exists.
2. **`workspace` / `project` scope clearing** — "the folder leaves the config set" is hard to observe. Is it cleared lazily on next resolve, or eagerly? Lazy is simpler and probably right; write it down.
3. **Does `recall` deserve a cache?** It runs every pass and a naive provider will re-scan a file each time. The clamp bounds the output, not the work. Consider a kernel-side per-pass memo keyed by `(provider, scopes, query)`.
4. **Is substring+recency retrieval too weak to be worth shipping?** It is a reference implementation, not a recommendation. If it embarrasses us, ship it as `examples/` rather than `extensions/crates/`. Revisit after the first real user.
