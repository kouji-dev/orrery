# orrery-ext-git

Git tools over gitoxide, reusing the knowledge in ade/src-tauri/src/git/.

**Manifest field.** `tools` — this crate is a first-party implementation of
`ExtensionDefinition.tools`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Implemented, read-only: `status`, `log`, `show`, `diff`, `blame`.
Implementation plan:
[`harness/docs/plans/06-extension-host.md`](../../../docs/plans/06-extension-host.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

| Capability | Why |
|---|---|
| `read = ["$WORKSPACE/**"]` | It reports on the tree: status, log, show, diff, blame. Nothing outside the workspace is any of its business. |

`write` and `spawn` were in the scaffold's manifest and are **not** here, because
nothing in this crate uses them and a grant nothing uses is a capability handed
over for nothing. They come back with the write verbs:

- `write = ["$WORKSPACE/.git/**"]` for staging and committing — the object
  database and the index, **and nothing else**. The worktree stays unwritable:
  that is the difference between "this can commit" and "this can edit", and the
  second is what `builtin.write` is for, under its own grant.
- `spawn = ["git"]` for push, fetch, clone and pull only, because they need
  credential helpers, proxies and SSH agents that gitoxide does not speak.

## How a policy check happens when gitoxide uses `std::fs`

gitoxide opens the object database itself, which the broker cannot mediate. So
before any verb touches a repository, the bundle asks the broker to read the
repository marker — a real, policy-checked `read` call against that path. A
**denial** stops the verb and becomes `Outcome::Denied`, and lands in the ledger.
Anything else does not, because the probe asks "may I", not "is it there".

## Tests

```
cargo test -p orrery-ext-git
```

Fourteen cases. The refusal path runs against the mock broker in
`orrery-ext-api::testing`; the five verbs run against a real repository built in
a temp directory with the system `git` binary — deliberately not with gitoxide,
because a test that built its commits with the library under test could not catch
a library that writes and reads its own mistake consistently.

No network, and nothing outside the temp directory.
