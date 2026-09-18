# orrery-ext-session-sqlite

The default session backend: SQLite in WAL mode, one transaction per turn append, a writer actor per session.

**Manifest field.** `session` — this crate is a first-party implementation of
`ExtensionDefinition.session`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## How it is put together

| Piece | Where | Why |
|---|---|---|
| Writer actor | `src/writer.rs` | One per session, on its own OS thread with its own write connection. SQLite takes one writer at a time; serialising on purpose beats discovering `SQLITE_BUSY` under load. Every op is **one transaction**, and a failed op is a value on a `oneshot`, never the end of the loop. |
| Read path | `src/read.rs` | A second, **read-only** connection under `spawn_blocking`. WAL is what lets a long `materialise` run without stalling the turn appending beside it. |
| Schema | `src/schema.rs` | `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, a `busy_timeout`. `payload` is a `serde_json` blob; a `meta.schema_version` row gates future migrations. |
| Conversions | `src/convert.rs` | Ids are stored as uuid strings, so a session database is readable with `sqlite3`. A constraint violation comes back as `SessionError::Corrupt` — a value somebody can be told about — never a panic. |

Sequence numbers are **not** allocated here. They come from the
`BranchLease` in `orrery-session`, and the branch's counter only moves once the
transaction has committed, so a failed write leaves no gap.

`close_branch` writes the child's `branches` row and nothing else. The parent
appends its own `BranchResult` under its own lease. See the invariant at the top
of `orrery_session::lease`.

## Tests

```
cargo test -p orrery-ext-session-sqlite
```

| File | Covers |
|---|---|
| `tests/schema.rs` | The DDL is idempotent, the pragmas are set, a duplicate `(branch, seq)` is `Corrupt`. |
| `tests/conformance.rs` | The whole `orrery_session::conformance` suite, plus the compaction assertions that need to read the rows directly. |
| `tests/concurrency.rs` | 100 appends from 10 tasks come out as seq 1..100 with no gaps; the actor survives a failed op. |
| `tests/crash.rs` | A real child process (`examples/crash_child.rs`) is killed mid-run — `TerminateProcess` on Windows — and the reopened database has every acknowledged turn, at most one unacknowledged one missing, and still materialises. |

Implementation plan: [`harness/docs/plans/02-session-store.md`](../../../docs/plans/02-session-store.md).
