# orrery-ext-git

Git tools over gitoxide, reusing the knowledge in ade/src-tauri/src/git/.

**Manifest field.** `tools` — this crate is a first-party implementation of
`ExtensionDefinition.tools`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Scaffold only. Implementation plan:
[`harness/docs/plans/06-extension-host.md`](../../../docs/plans/06-extension-host.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

| Capability | Why |
|---|---|
| `read = ["$WORKSPACE/**"]` | It reports on the tree: status, diff, blame. Nothing outside the workspace is any of its business. |
| `write = ["$WORKSPACE/.git/**"]` | Staging and committing write the object database and the index — **and nothing else**. The worktree is deliberately not writable here: a git tool that could rewrite your files is a much larger thing to trust. |
| `spawn = ["git"]` | Four operations only — push, fetch, clone, pull — because they need credential helpers, proxies and SSH agents that gitoxide does not speak. Everything else is in-process. |

The narrow `write` is the interesting line. It is the difference between "this can
commit" and "this can edit", and the second is what `builtin.write` is for, under
its own grant.

## Tests

```
cargo test -p orrery-ext-git
```

Nothing yet: this crate is a scaffold. When it lands, its tests run against the
mock broker in `orrery-ext-api::testing`, like every other extension — no real
repository, no network.
