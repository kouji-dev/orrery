# orrery-ext-tools-builtin

The built-in tool bundle: read, write, edit, bash, grep and glob, shipped as a first-party extension like any other.

**Manifest field.** `tools` — this crate is a first-party implementation of
`ExtensionDefinition.tools`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Status.** Implemented. Implementation plan:
[`harness/docs/plans/06-extension-host.md`](../../../docs/plans/06-extension-host.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

| Capability | Why |
|---|---|
| `read = ["$WORKSPACE/**"]` | `read`, `grep` and `glob` are the tools that see the tree. `glob` and `grep` discover names through the broker's `list`, not `std::fs::read_dir`, so a path `read` would refuse is not even named. |
| `write = ["$WORKSPACE/**"]` | `write` and `edit`. Both are atomic, so a cancel reverts rather than truncates. |
| `spawn = ["*"]` | `bash` runs what the model wrote. The set cannot be enumerated in advance, which is exactly why it is the most policy-narrowed tool in the system. |

This is the widest request in the tree, and it is one manifest covering six tools
on purpose. Each tool declares its **own** `requires`, so a grant that withholds
`spawn` disables `bash` and leaves the other five working, and the ledger says
which. A bundle asking for the union of its tools is not the same as any one call
needing that union.

## Tests

```
cargo test -p orrery-harness --test builtin
```

The suite lives in the facade rather than here, because it exercises these tools
through the real registry, the real policy engine and a real process tree — which
means naming `orrery-kernel`, and `deps-check` rule 3 forbids an extension crate
from doing that. The extension is tested the way it is used.
