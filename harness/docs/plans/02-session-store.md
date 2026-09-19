# 02 · Session — the turn tree and the only place history lives

**Goal.** A turn tree that survives a crash losing at most one turn, replays exactly, forks for sub-agents and retries, and compacts without ever destroying what it compacted. Two crates: the trait plus the pure algebra plus a conformance suite (`orrery-session`), and the shipped SQLite backend (`orrery-ext-session-sqlite`). When this is done a session can be killed mid-turn, reopened, and materialised into the same messages.

**Covers.** §4.2 in full · the `compact` half of §4.6 · the storage half of §4.14's "a failed eval case is openable".

**Crates.** `core/crates/orrery-session` (published, trait + algebra + conformance) · `extensions/crates/orrery-ext-session-sqlite` (the `session` singleton).

**Depends on.** [`01-proto-shared-types.md`](01-proto-shared-types.md).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **The lease.** One turn at a time per branch; parallel work is parallel branches; `append` takes a `&BranchLease` it cannot forge; a second submit on a busy branch is `SessionError::BranchBusy`, not a queue.
- **The parent performs the join.** A child branch cannot append to its parent. Cross-referencing this here because it is the one deadlock in the design.
- SQLite WAL, `synchronous=NORMAL`, one transaction per append, one writer actor per session.
- Turns are immutable. `compact` writes, never mutates.
- `SessionStore` is `Arc<dyn>`, `#[async_trait]`.
- A `SessionStore` failure **ends the session** (§4.7) — it is the one singleton that cannot degrade, because history is the one thing that cannot be reconstructed.

This crate owns translation **#6** jointly with plan 03 (`materialise` needs a `TokenCounter`).

---

## Architecture

### The trait

```rust
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn create(&self, workspace: &str, profile: &str) -> Result<SessionId, SessionError>;
    async fn open(&self, session: SessionId) -> Result<SessionHandle, SessionError>;

    /// Acquire the right to append. try_lock, never lock: a busy branch is refused.
    async fn lease(&self, branch: BranchId) -> Result<BranchLease, SessionError>;

    async fn append(&self, lease: &BranchLease, turn: NewTurn) -> Result<TurnId, SessionError>;
    async fn branch(&self, from: TurnId, label: &str) -> Result<BranchId, SessionError>;
    async fn close_branch(&self, lease: BranchLease, outcome: BranchOutcome) -> Result<(), SessionError>;

    async fn materialise(&self, branch: BranchId, budget: TokenBudget, counter: &dyn TokenCounter)
        -> Result<Materialised, SessionError>;
    async fn compact(&self, lease: &BranchLease, upto: Seq, summary: NewTurn)
        -> Result<CompactResult, SessionError>;

    /// Replay for `session.attach(since)` and `orrery replay`.
    async fn events_since(&self, session: SessionId, since: Option<Seq>)
        -> Result<Vec<StoredEvent>, SessionError>;
}
```

`materialise` takes the counter as an argument rather than storing one: the store must not know which provider is bound, and the same branch materialises differently for two models. `Materialised` carries the messages **and** what was dropped:

```rust
pub struct Materialised {
    pub messages: Vec<Message>,
    pub tokens_estimated: u64,
    pub elided: Vec<TurnId>,          // what the budget cut — the caller may want to compact instead
    pub watermark: Option<Seq>,       // the compaction this view sits on
}
```

### The lease

```rust
pub struct BranchLease {
    guard: OwnedMutexGuard<BranchState>,      // private
    branch: BranchId,
}
impl BranchLease { pub fn branch(&self) -> BranchId; }
// No Clone. No Default. No public constructor. Only `SessionStore::lease` makes one.
```

`BranchState` holds the next `Seq` and the branch's status. The registry is a `DashMap<BranchId, Arc<Mutex<BranchState>>>` in the *trait* crate, not the backend, so every backend inherits the concurrency rule rather than reimplementing it.

### Turn kinds

```rust
#[non_exhaustive]
pub enum TurnKind {
    User        { input: UserInput },
    Assistant   { content: Vec<ContentBlock>, usage: Usage },
    ToolResult  { call: CallId, r#ref: ToolRef, outcome: Outcome },
    /// Written by the PARENT when a child branch closes. The child's turns stay in place.
    BranchResult{ child: BranchId, outcome: BranchOutcome },
    /// Written by `compact`. Never replaces anything; a watermark row points at it.
    Summary     { covers: (Seq, Seq), text: String, usage: Usage },
    /// Memory recalled at context.build, recorded as RESOLVED CONTENT (§4.3).
    Recalled    { provider: String, entries: Vec<RecalledEntry> },
}
```

`Recalled` is why memory is replayable: the store moves on, the tree still shows what the model saw.

### Materialise

Pure, sync, and therefore testable without a database:

```rust
pub mod algebra {
    /// Walk ancestry, apply the highest watermark, replay above it, fit the budget.
    /// Sync and pure: the backend hands it rows, it hands back messages.
    pub fn materialise(rows: &[TurnRow], watermark: Option<Seq>,
                       budget: TokenBudget, counter: &dyn TokenCounter) -> Materialised;
}
```

Order of operations, and it matters for provider caching (§4.3's assembly rule is plan 05's, but the *history* half is here): oldest-first, never reordered; elision takes from the middle, never the head, because the head is the cached prefix; the most recent turn is never elided.

### SQLite backend

```sql
CREATE TABLE sessions  (id TEXT PRIMARY KEY, workspace TEXT, profile TEXT, created_at INTEGER);
CREATE TABLE branches  (id TEXT PRIMARY KEY, session TEXT NOT NULL, parent_branch TEXT,
                        forked_at_turn TEXT, label TEXT, state TEXT NOT NULL);
CREATE TABLE turns     (id TEXT PRIMARY KEY, branch TEXT NOT NULL, seq INTEGER NOT NULL,
                        kind TEXT NOT NULL, payload BLOB NOT NULL, created_at INTEGER,
                        UNIQUE(branch, seq));
CREATE TABLE compactions(branch TEXT NOT NULL, upto_seq INTEGER NOT NULL, summary_turn TEXT NOT NULL,
                        PRIMARY KEY (branch, upto_seq));
CREATE TABLE events    (session TEXT NOT NULL, seq INTEGER NOT NULL, payload BLOB NOT NULL,
                        UNIQUE(session, seq));
CREATE INDEX turns_branch_seq ON turns(branch, seq);
```

`payload` is a `serde_json` blob, not columns: the turn shape will evolve and a migration per field is not worth it. A `schema_version` row in a `meta` table gates future migrations.

The writer actor:

```rust
enum WriteOp { Append { .. , reply: oneshot::Sender<Result<TurnId, SessionError>> }, .. }
// one task per session, owns the write connection, loops on an mpsc::Receiver<WriteOp>
```

Reads use a second, read-only connection under `spawn_blocking`. WAL makes them concurrent with the writer.

---

## File structure

**Create**

- `harness/core/crates/orrery-session/src/{lib,trait,lease,turn,algebra,error}.rs`
- `harness/core/crates/orrery-session/tests/conformance.rs` — the reusable suite, exported as a `pub fn run_conformance(store: Arc<dyn SessionStore>)` so any backend runs it
- `harness/core/crates/orrery-session/tests/algebra.rs`
- `harness/extensions/crates/orrery-ext-session-sqlite/{Cargo.toml,orrery.toml,README.md}`
- `harness/extensions/crates/orrery-ext-session-sqlite/src/{lib,schema,writer,read,convert}.rs`
- `harness/extensions/crates/orrery-ext-session-sqlite/tests/{conformance,crash,concurrency}.rs`

---

## Tasks

### Task 1 · Turn types and the algebra

Files: `src/turn.rs`, `src/algebra.rs`, `tests/algebra.rs`

- [x] **Failing test first.** `algebra::materialise_is_oldest_first` — three turns in, three messages out, in order.
- [x] `algebra::elision_never_drops_the_head` — a budget that fits only two of five turns keeps turn 1 and turn 5.
- [x] `algebra::watermark_replaces_the_prefix` — with a watermark at seq 3 and a summary turn, the output is summary + turns 4,5, and turns 1–3 are absent but still in the input rows.
- [x] Implement `TurnKind`, `TurnRow`, `NewTurn`, `Materialised`, `algebra::materialise`.
- [x] `TokenCounter` trait here (`fn count(&self, messages: &[Message]) -> u64`) with a `CharsOverFour` test impl. The real ones are plan 03.

### Task 2 · The lease

Files: `src/lease.rs`, `tests/conformance.rs`

- [x] **Failing test first.** `conformance::second_lease_is_refused` — take a lease, `lease()` the same branch again, assert `SessionError::BranchBusy`; drop the first, assert the second now succeeds.
- [x] `conformance::two_branches_lease_concurrently` — two leases on different branches held at once.
- [x] Implement `BranchLease`, `BranchState`, the `DashMap` registry, `lease()` on `try_lock_owned`.
- [x] **A compile-fail test**: constructing a `BranchLease` outside the crate does not compile, and neither does cloning one. This is the whole point of the type; prove it.
  - **Done with `compile_fail` doctests, not `trybuild`.** A doctest links the crate as an external dependency, which is exactly the vantage point the test needs, and it costs no new dependency — `trybuild` is not in the workspace pins or the lockfile, and adding one to a `Cargo.lock` shared with two other agents mid-flight is not worth it for a guarantee we already have. The two cases are at the top of `src/lease.rs`.

### Task 3 · The trait and the conformance suite

Files: `src/trait.rs`, `tests/conformance.rs`

- [x] Write `run_conformance(store)` — it lives in **`src/conformance.rs`**, not `tests/`, because a `tests/` binary cannot be linked by a backend crate and the whole point is that both backends run the same code. `tests/conformance.rs` is the memory store plus the runner. Covering: create → append → materialise; branch → append on child → close → parent sees `BranchResult`; append out of order rejected (as `conformance::appends_are_contiguous` — the lease is the only source of a `Seq`, so "out of order" is only reachable by going under the trait, which `sqlite::unique_branch_seq_is_enforced` and `concurrency::writer_survives_a_failed_op` do); `events_since` returns a contiguous `seq` range; `compact` leaves the original rows readable.
- [x] Add an in-memory `Vec`-backed store **in the test module only**, so the suite is exercised before the SQLite backend exists. It is not a shipped backend — it exists to prove the suite runs.

### Task 4 · SQLite schema and reads

Files: `orrery-ext-session-sqlite/src/{schema,read}.rs`

- [x] **Failing test first.** `sqlite::schema_applies_and_is_idempotent` — apply twice, no error.
- [x] `sqlite::unique_branch_seq_is_enforced` — two rows with the same `(branch, seq)` is a constraint violation, surfaced as `SessionError::Corrupt`, not a panic.
- [x] Apply the DDL, set `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`.
- [x] Read path on a read-only connection under `spawn_blocking`.

### Task 5 · The writer actor

Files: `orrery-ext-session-sqlite/src/writer.rs`, `tests/concurrency.rs`

- [x] **Failing test first.** `concurrency::appends_on_one_branch_are_ordered` — 100 appends from 10 tasks holding the lease in turn produce seq 1..100 with no gaps.
- [x] `concurrency::writer_survives_a_failed_op` — one op returns an error; the next succeeds (the actor does not die on a single failure).
- [x] Implement the `mpsc` + `oneshot` actor, one transaction per append.

### Task 6 · Crash durability

Files: `tests/crash.rs`

- [x] **Failing test first.** `crash::loses_at_most_one_turn` — spawn a child process that appends N turns and is SIGKILLed mid-run; reopen the database; assert the turn count is N or N-1 and that `materialise` succeeds. On Windows use `TerminateProcess` via the existing job-object helper.
- [x] Assert the WAL recovers without manual intervention.

### Task 7 · Compaction

Files: `src/algebra.rs`, `orrery-ext-session-sqlite/src/lib.rs`

- [x] **Failing test first.** `compact::original_rows_survive` — compact up to seq 3, then read `turns` directly and assert seqs 1–3 are still there.
- [x] `compact::materialise_uses_the_highest_watermark` — two compactions, the later one wins.
- [x] Implement `compact` as: write the summary turn, write the watermark row, one transaction.

### Task 8 · The parent-join deadlock

Files: `tests/conformance.rs`

- [x] **Failing test first, and this is the highest-value test in the suite.** `conformance::child_close_does_not_deadlock_the_parent` — parent holds its lease, spawns a child branch, the child appends and closes; assert the whole thing completes within a timeout and the parent's `BranchResult` row exists. Written before the implementation, it will hang; that is the point.
- [x] Implement `close_branch` so it writes only to the **child's** rows and returns the outcome; the parent writes `BranchResult` itself, under its own lease.
- [x] Document the invariant at the top of `lease.rs`.

### Task 9 · Manifest and registration

Files: `orrery-ext-session-sqlite/orrery.toml`

- [x] Write the manifest: `runtime = "native"`, `[provides] session = "sqlite"` (the singleton field names its implementation, which is what the scaffold already wrote and what lets a config layer say which one wins), `[requires] read/write = ["$STATE/**"]`.
- [x] Register through `orrery-host` like any extension. **Done (2026-09-19).** `orrery_ext_session_sqlite::SqliteSessions` implements `NativeExtension`, so the host parses this bundle's `orrery.toml` with the parser it holds a third party to, the `session` singleton has a named holder, and a deny rule can name `sqlite`. `build(state_dir)` stays as the direct constructor for an embedder that has already chosen SQLite - the same two-halves split `orrery-ext-views-default` has, because neither a session store nor a view is a tool the model can call. `tests/loader.rs` drives the real manifest parser.

---

### Task 10 · Enumeration

Files: `orrery-session/src/{trait,turn,conformance}.rs`,
`orrery-ext-session-sqlite/src/{lib,read}.rs`

**Added 2026-09-18, by plan 17.** `SessionStore` had `open(id)` and no way to
ask *which* sessions exist, so `orrery session list` had nothing to call. This
plan owns the trait, so the method landed here rather than in the CLI.

- [x] **Failing test first.** `conformance::sessions_can_be_enumerated` — every
  field a listing prints is asserted: id, workspace, profile, `created_at` and
  the turn count across every branch, newest first. It failed against both
  backends before either implemented it.
- [x] `list_sessions` on the trait, returning `SessionSummary`.
- [x] Implement it in `orrery-ext-session-sqlite`: one statement, with the turn
  count as a correlated subquery rather than a second round trip.

**The default implementation refuses, and refusing fails conformance.** The
method has a default that returns `SessionError::Backend` purely so an in-test
fake in `orrery-orchestrator` — a crate that wave did not own — kept
compiling. It is not a conformant implementation and cannot be one; delete the
default once that fake implements the method.

~~**Deleting is still not here.** `session rm` needs a delete, and open question 2
parks retention for the config phase; enumeration is a read and does not
prejudge it.~~ **Amended 2026-09-19: deleting is here.** `SessionStore::delete`
drops a **whole session** — its events, its turns, its compactions and its
branches, in one transaction — and the conformance suite's
`a_session_can_be_deleted` makes every backend provide it, including the part
that matters most: the *neighbouring* session is untouched. There is no
`delete_turn` and there will not be one; see open question 2 below.

---

## Done when

- `cargo test -p orrery-session` and `cargo test -p orrery-ext-session-sqlite` are green.
- The conformance suite passes against both the in-test memory store and SQLite.
- The crash test demonstrably loses ≤ 1 turn.
- The parent-join test completes rather than hanging.

## Open questions

1. **Does `events_since` belong on `SessionStore`?** It is the replay half of §5.3, and it is the only method a *transport* calls. Alternative: a separate `EventLog` trait backed by the same database, so a store backend does not have to implement replay. Decide before plan 08 consumes it.

   **Decided: it stays on `SessionStore`.** The event row and the turn row are written in the *same transaction* — that is what makes it impossible to replay an event for a turn that is not there, and impossible for the session-wide `seq` to skip a number. A separate `EventLog` trait would either need to share the store's write transaction (so it is not separable) or write in a second one (so a crash between the two is a lost or orphaned frame, and the gap-detection-by-arithmetic contract dies). The cost is one extra method a backend must implement; the alternative costs the contract. Revisit at plan 08 only if a transport turns up that wants replay from something that is not the store.

2. **Retention.** §4.2 says nothing about deleting sessions. The ADE's `history/mod.rs` has a retention policy worth copying. Out of scope for phase 1; note it for phase 5 (config).

   ~~**Confirmed out of scope.** Nothing in this plan deletes a row.~~ **Answered 2026-09-19, and implemented: the unit of deletion is the whole session.** The constraint the earlier note recorded turned out to *be* the answer — `materialise` walks ancestry, so a branch whose parent's prefix was trimmed cannot be replayed — and the cheapest way to make that unrepresentable is to give the trait one method that can only take a session. `SessionStore::delete(session)` is that method; there is no `delete_turn`, no `trim`, and no `--before` flag on `orrery session rm`, because a transcript with a hole in it still reads as a complete record and is not one. `compact` remains the only way a branch gets shorter, and it writes rather than mutates.

   Three consequences, all tested. The default implementation on the trait *refuses*, so a backend that has not thought about deletion fails `conformance::a_session_can_be_deleted` rather than silently dropping nothing. Deleting a session that is not there is `NoSuchSession`, not a quiet success, so a caller can tell "gone now" from "was never here". And the sqlite backend drops the in-memory lease registry entries and the per-session writer actor with the rows, because leaving a writer thread open on a session that no longer exists is a leak with a plausible-looking name.

   What is still **not** here is a retention *policy* — an age, a count, a size that decides which sessions go. That is still phase 5's, and it is now a loop over `list_sessions` and `delete` rather than a missing capability.

3. **`Materialised.elided`** — is exposing what was cut useful to the caller, or does it invite the kernel to second-guess the store? Keep it for now because plan 05's compaction trigger wants it.

   **Decided: kept.** `elided` is what tells plan 05 "you are paying to drop history every turn; compact instead". Without it the trigger has to guess from token counts. It is a report, not a lever: nothing in the API lets a caller put an elided turn back, so it cannot be used to second-guess the store, only to decide to compact.

## What was built

Both crates are green:

```
cargo test -p orrery-session            # 5 algebra + 6 conformance + 3 doctests
cargo test -p orrery-ext-session-sqlite # 4 schema + 7 conformance + 3 concurrency + 1 crash
```

Two things worth knowing that are not obvious from the task list:

- **`algebra::materialise` yields when the last turn alone will not fit.** "Never the head" and "never the most recent" are both in the plan, and a two-turn branch under a one-token budget cannot honour both. The most recent turn wins: dropping what the model just did is how a loop starts. `tests/algebra.rs::the_most_recent_turn_survives_any_budget` pins it.
- **A superseded summary is dropped from the view, not hoisted.** With two compactions, the view shows the summary the *winning* watermark points at and nothing from the earlier one — both rows are still on disk, and `tests/conformance.rs::materialise_uses_the_highest_watermark` asserts all eight rows are there.

---

## State

**Landed.** SQLite in WAL mode, one transaction per turn append, a writer actor
per session, and the conformance suite in `orrery-session/src/conformance.rs`
run by both the in-memory store and the SQLite one — which is what makes the
suite a contract rather than a description of one implementation.

**Amended (2026-09-19): the last task is genuinely done.** "Register through
`orrery-host` like any extension" had been ticked with a `TODO(plan-06)` and a
direct constructor standing in, and the loader had existed for several waves.
`orrery_ext_session_sqlite::SqliteSessions` now implements `NativeExtension`, so
this bundle's `orrery.toml` goes through the same parser a third party's does,
the `session` singleton has a named holder in the ledger, and a deny rule can
name `sqlite`. `build(state_dir)` stays beside it as the direct constructor for
an embedder that has already chosen SQLite — the same two-halves split
`orrery-ext-views-default` has, because neither a session store nor a view is a
tool the model can call. `tests/loader.rs` drives the real manifest parser.

Crash safety is covered by a real killed child process (`tests/crash.rs` plus
`examples/crash_child.rs`), not by a simulated one.
