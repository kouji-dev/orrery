# Writing an Orrery extension

Everything you need to build, test, publish and pin an extension — and nothing in
here is a special path for us. Every crate under `crates/` is written as if a
third party owned it, so a community extension is literally the same crate in
somebody else's repository. The path is exercised daily rather than discovered by
the first outside contributor.

**Any language takes the same five steps.** A Rust, Go, Python or JavaScript
extension changes only the `runtime` line and the binding style — same manifest,
same grant flow, same registry, same namespacing. The kernel does not know which
runtime it dispatched to, and
[`orrery-harness/tests/parity.rs`](../core/crates/orrery-harness/tests/parity.rs)
proves it: one logical tool, four runtimes, one `Surface`.

---

## The five steps

### 1 · Declare

The manifest is the contract. **Nothing outside it is available at runtime.**

```toml
api     = "orrery-ext/1"
runtime = "node"                     # native | wasm | node | python | process

[extension]
id      = "buildgraph"
version = "1.2.0"

[provides]
tools = ["impacted", "deps"]         # → buildgraph.impacted, buildgraph.deps

[requires]
read  = ["$WORKSPACE/**"]
spawn = ["java"]
```

Tools are namespaced by the extension id, always. Two extensions cannot claim the
same tool name, and a name in a transcript says where it came from.

#### What `[provides]` can hold

Collections — many extensions may contribute to each:

| Field | What it contributes | Documented by |
|---|---|---|
| `tools` | Callable tools | [`06-extension-host`](../docs/plans/06-extension-host.md) |
| `providers` | Model providers | [`03-provider-layer`](../docs/plans/03-provider-layer.md) |
| `agents` | Role agents: planner, executor, … | [`11-router-roles-orchestrator`](../docs/plans/11-router-roles-orchestrator.md) |
| `workflows` | Multi-step orchestrations | [`11-router-roles-orchestrator`](../docs/plans/11-router-roles-orchestrator.md) |
| `interceptors` | Hooks around a call | [`04-tool-registry`](../docs/plans/04-tool-registry.md) |
| `lifecycle` | Session start/stop handlers | [`06-extension-host`](../docs/plans/06-extension-host.md) |
| `graders` | Eval verdicts | [`16-eval-runner`](../docs/plans/16-eval-runner.md) |
| `commands` | CLI subcommands | [`17-cli`](../docs/plans/17-cli.md) |
| `views` | Loop event → surface bindings | [`09-surfaces`](../docs/plans/09-surfaces.md) |
| `renderers` | Client-side drawing of a custom surface | [`09-surfaces`](../docs/plans/09-surfaces.md) |
| `skills` | Prompt-level capabilities | [`13-skills-mcp`](../docs/plans/13-skills-mcp.md) |
| `mcp` | MCP servers exposed as tools | [`13-skills-mcp`](../docs/plans/13-skills-mcp.md) |

Singletons — **exactly one** extension may hold each, and a second claim is a
load error rather than a silent override:

| Field | What it replaces |
|---|---|
| `memory` | The memory backend |
| `session` | The session store |
| `permissions` | The permission decision point |
| `router` | Model routing |

The field list is the design rule: **if a subsystem can be replaced, it is a
field here. If it is not a field, it is ours and it is not replaceable.** The
loop is not a field.

### 2 · Implement

The extension gets a context object. It cannot import `fs` or `child_process` —
the equivalents arrive through `ctx`, and every one goes back through the broker:
policy-checked, budgeted, audited, cancellable.

```js
export default defineExtension({
  tools: {
    impacted: {
      description: "Which modules a change touches.",
      input: { type: "object", properties: { rev: { type: "string" } } },
      requires: ["spawn"],
      async run({ rev }, ctx) {
        const out = await ctx.proc.run("java", ["-jar", "buildgraph.jar", rev]);
        return ctx.ui.table({ columns: ["module", "reason"], rows: parse(out.stdout) });
      },
    },
  },
});
```

Two things in there are worth the words:

- **`requires: ["spawn"]`** is per tool, not per extension. Without a `spawn`
  grant this one tool is *disabled* — never offered to the model, refused with a
  reason if something asks anyway — and the rest of the extension keeps working.
- **`ctx.ui.table(...)` describes a table rather than drawing one.** ratatui
  renders a widget, Ink renders a component, `--json` emits the payload, all from
  the same description. An extension that drew would have to know which client it
  was talking to, and then there would be extensions that only work in one.

Worked examples, one per runtime, all returning the same thing:

| Example | Runtime | Bindings |
|---|---|---|
| [`examples/native-hello`](examples/native-hello) | `native` | `orrery-ext-api`, compiled in |
| [`examples/node-hello`](examples/node-hello) | `node` | `@orrery/ext` |
| [`examples/wasm-hello-rs`](examples/wasm-hello-rs) | `wasm` | `orrery-guest` |
| [`examples/wasm-hello-go`](examples/wasm-hello-go) | `wasm` | raw `.wit` + `wit-bindgen`, no SDK of ours |

The Go one is the one that matters. A language Orrery ships nothing for binds
against the world with `wit-bindgen` alone; if that is painful, the WIT is wrong.

### Describing, never drawing: the ported three

Phase 4's acceptance criterion (plan 09, §8) is three ported extensions that
render in both TUIs and in `--json` **with no drawing code of their own**:

| Example | Emits |
|---|---|
| [`examples/workspace-census`](examples/workspace-census) | a section, a markdown summary, a table, styled text |
| [`examples/patch-review`](examples/patch-review) | a diff and a question, with a `default` an unattended run resolves |
| [`examples/release-train`](examples/release-train) | a task list, progress, and one `custom` surface with an informative fallback |

All twelve core surfaces are reachable from the published crate: `ctx.ui` is
[`SurfaceSink`](../core/crates/orrery-ext-api/src/ctx.rs) plus the
[`SurfaceBuilders`](../core/crates/orrery-ext-api/src/sink.rs) trait. The proof
that they draw nothing is mechanical rather than rhetorical:
`clients/ported/tests/ported.rs` scans their source for `std::io`, `print!`, a
terminal crate or a width and fails on any of them, and
`clients/ported` replays what they emit through every renderer.

### 3 · Grant

Installing surfaces the manifest's requests as a diff the user or admin approves.
**Denying does not fail the install** — the affected tool is disabled, everything
else keeps working, and the ledger records `degraded` with the reason.

### 4 · Scope

Where it applies is configuration, not code: enabled globally, per workspace, or
per project, and optionally restricted to named sub-agents. Layer precedence
decides between two extensions that claim the same thing.

### 5 · Distribute

Publish to crates.io or npm for the community, or to the organisation's signed
registry for internal use. An enterprise pins the version and the signature, and
the harness refuses to load anything unpinned when managed config says so
([plan 15](../docs/plans/15-registry-supply-chain.md)).

---

## Installing one, and what the registry does not cover

`orrery install <source>` takes seven forms (plan 15's Architecture table).
**Only one of them is verified**: a bare registry name resolves against a signed
index, which pins a `sha256` and mirrors the manifest's `requires` so a capability
change is visible without downloading the package. The other six — `github:`, a
git URL, `crate:`, `npm:`, a local path, and `--link` — have no signature and no
hash to check against, so they install `pinned: false`, say so in the supply-chain
ledger, and are refused outright under a managed `registry.unpinned = "refuse"`.

The rule in one line: **the registry is how you trust an extension; the other
sources are how you try one.**

---

## The threat model, honestly

`wasm` is a real boundary. A guest is isolated by construction, there are **zero
WASI preopens**, and the only way out is an imported host function that the
policy engine answers.

`native`, `node`, `python` and `process` are **capability-withholding, not
sandboxing** (translation #13). We do not hand a node extension a filesystem
handle, and the SDK's loader hook makes `require('fs')` fail — but that hook is a
convenience, not a boundary. A determined node extension still shares the process
and can reach around it. Saying otherwise in a code comment and nowhere else
would be the bug, so it is said here.

The registry does not close it either. A git or local install pulls a
**transitive dependency tree nobody here reviews**. We pin the extension; its
crates.io or npm dependencies are pinned by its own lockfile, which is not part
of the review. A malicious transitive dependency is still a hole, and the wasm
runtime is the only one that closes it.

---

## The rules that keep this real

Enforced mechanically, not by review: `cargo xtask deps-check` and
`cargo xtask publish-check`.

### Crate layout

```
orrery-ext-<name>/
├─ Cargo.toml        # publish = true; deps on core carry BOTH version and path
├─ orrery.toml       # the manifest — the same one a third party ships
├─ README.md         # what it provides, what it asks for and why, how to test it
├─ CHANGELOG.md
└─ src/lib.rs
```

### The dependency rule

```toml
orrery-ext-api = { version = "0.1", path = "../../../core/crates/orrery-ext-api" }
```

`version` makes it publishable; `path` makes it build in-tree. **When
`extensions/` moves to its own repo, delete the `path` half. Nothing else
changes.**

Extensions may depend only on the **published** core crates: `orrery-proto`,
`orrery-ext-api`, `orrery-provider`, `orrery-session`, `orrery-memory`,
`orrery-grader`. Everything else in `core/` is `publish = false` — kernel
internals are not an API.

No extension reaches into `core/` for anything else: no shared `build.rs`, no
`include!` across the boundary, no dev-dependency on `orrery-kernel`.

### Testing

Tests use `orrery-ext-api::testing` — the mock broker, which is **the same code
path `orrery ext test` uses**. A community author has the identical harness,
including the same ledger and the same denials a real session produces. No model,
no network.

```
orrery ext test path/to/your/extension
```

---

## README template

Every crate under `crates/` carries one of these. Copy it.

```markdown
# orrery-ext-<name>

<One sentence: what it is.>

**Manifest field.** `<tools|providers|…>` — it loads through `orrery-host` like
any third-party extension: the ledger shows it, a deny rule disables it,
`orrery ext test` runs it.

## What it provides

| Tool | What it does |
|---|---|
| `<ext>.<tool>` | … |

## What it asks for, and why

| Capability | Why |
|---|---|
| `read = ["$WORKSPACE/**"]` | <The reason. Not "to read files" — *which* files, and what breaks without them.> |

## Tests

```
cargo test -p orrery-ext-<name>
```

<What the suite covers, and what it deliberately does not touch.>
```

The "and why" column is the part that is easy to skip and the part that matters.
A capability list with no reasons is a list nobody can review, and an install
prompt is only meaningful if somebody could have said no.

---

## Publishing

### Release order

Core trait crates first, then anything that names them. Within core, dependency
order:

1. `orrery-proto`
2. `orrery-ext-api`
3. `orrery-provider`, `orrery-session`, `orrery-memory`, `orrery-grader`
4. `orrery-guest` (the wasm guest SDK)
5. extensions

Publishing out of order fails at the upload, because crates.io resolves every
dependency at that moment — which is exactly what the dry run below catches
first.

### What checks it

| Command | When | Network |
|---|---|---|
| `cargo xtask publish-check` | Every test run, every CI build | No |
| `cargo publish --dry-run -p <crate>` | CI only | Yes |

`publish-check` is the offline half: the preconditions cargo evaluates from the
manifests alone — a path dependency with no `version`, a dependency on a
`publish = false` crate, and the fields crates.io refuses an upload without.
Those are the failures we can cause by accident. The real dry run resolves
against the index and therefore needs the network, so it is a CI step: nothing in
this repository's test suite opens a socket.

---

## The split checklist

`extensions/` becomes its own repository eventually. Every item below is
verifiable **today**, so the move is mechanical when it happens rather than a
project.

- [ ] `cargo xtask deps-check` green — proves no `path`-only or unpublished
      dependency remains. *Verifiable now:* it runs in CI.
- [ ] `cargo xtask publish-check` green, and `cargo publish --dry-run` green for
      every `publish = true` crate. *Verifiable now:* both run in CI.
- [ ] Every published core crate is actually on crates.io at the version the
      extensions name. *Verifiable now:* the dry run fails if it is not.
- [ ] `extensions/` gets its own `Cargo.toml` `[workspace]` and its own
      `pnpm-workspace.yaml` entry. *Verifiable now:* `examples/wasm-hello-rs`
      already carries a bare `[workspace]` and builds standalone, which is the
      same move.
- [ ] The root workspace drops `harness/extensions/crates/*` and
      `harness/extensions/examples/native-hello` from `members` and
      `default-members`.
- [ ] CI jobs for `extensions/` move with it. *Verifiable now:* they are already
      separate steps rather than one `--workspace` run.
- [ ] `orrery-harness`'s feature-gated links become ordinary crates.io
      dependencies — `{ version = "0.1" }` with the `path` half deleted.
      *Verifiable now:* every one of them already carries both halves.
- [ ] `orrery-harness/tests/parity.rs` moves with the examples, or keeps its
      `native-hello` dependency as a crates.io one. It is the only core test that
      names an extension crate.
- [ ] The signed registry ([plan 15](../docs/plans/15-registry-supply-chain.md))
      index gains the new repo's provenance.
- [ ] Nothing in `core/` changes. **If something does, the boundary was wrong and
      this checklist found it.**

**When.** After phase 2 freezes the manifest and the WIT, not before: while the
contract is still moving, a single repo makes a breaking change to
`orrery-ext-api` a one-commit affair.

---

## Layout

| Path | What |
|---|---|
| `crates/` | First-party Rust extensions, `runtime = "native"`, plus `orrery-guest` (the wasm guest SDK). |
| `node/` | The `@orrery/ext` SDK and first-party JavaScript extensions. |
| `examples/` | One worked example per runtime, plus the three **ported** extensions phase 4 renders. |

Implementation plan:
[`../docs/plans/18-writing-an-extension.md`](../docs/plans/18-writing-an-extension.md).
