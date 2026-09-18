# orrery-ext-memory-file

The reference memory provider: file-backed, `global` and `session` scopes only.
**Not in any default feature set.**

**Manifest field.** `memory` — this crate is a first-party implementation of
`ExtensionDefinition.memory`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

## What it does

JSONL under the state directory — `$STATE/memory/global.jsonl` and
`$STATE/memory/session/<id>.jsonl` — with **substring plus recency** retrieval and
`forget` by selector. That is the whole strategy.

## What it does not do

Scoping, lifetime, visibility and the token clamp are the kernel's
(`orrery-memory`). This crate is handed a scope list that has already been filtered
and a write that has already been admitted. It declares the two scopes it keeps and
refuses the rest with `MemError::UnsupportedScope` — never with a silent success.

It passes `orrery_memory::conformance::run_conformance`, which is the same suite the
in-test provider runs.

## Why it is off by default

It is not good. A mediocre default memory is worse than none for evals, which is why
`EvalRun.memory` defaults to `"off"` (plan 16). It ships so that the conformance
suite has a real implementation to be meaningful, so that a first run is not simply
memoryless, and so the scope rules are concrete for anyone writing a better one.

Turning it on is an embedder's decision: `orrery-harness` would carry a
`memory-file` feature, and it would not be in that crate's `default` set.

See [`orrery.toml`](orrery.toml) for what it provides and what it requires, and
[`harness/docs/plans/12-memory.md`](../../../docs/plans/12-memory.md) for the design.
