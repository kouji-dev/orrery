# native-hello

The `native` worked example: one tool, compiled into the harness.

This is the fourth writing of the same logical tool. The other three are
[`../node-hello`](../node-hello) (a child process speaking JSON-RPC),
[`../wasm-hello-rs`](../wasm-hello-rs) and [`../wasm-hello-go`](../wasm-hello-go)
(components built from the same `.wit` by two languages). All four return the
same `Surface`, and `orrery-harness/tests/parity.rs` asserts it.

## What it provides

| Tool | What it does |
|---|---|
| `hello.parity` | Describes a fixed two-row table. Takes no input. |

`parity` is deliberately boring. It exists so that four runtimes can be compared
on a value rather than on a story, and anything interesting in it would be a
reason the four might legitimately differ.

## What it asks for, and why

**Nothing.** `[requires]` is empty, and that is load-bearing: the parity
comparison is about *dispatch*, so every runtime is granted the same thing —
nothing at all — and a difference between the legs cannot be a policy difference
in disguise.

## `native` is a runtime, not a back door

Nothing here is privileged. The manifest is the same TOML a third party ships,
read by the same parser. The call arrives through the same `ExtensionTable`, is
policy-checked by the same engine, and is bounded by the same `ToolBudget`. The
only difference between this crate and `node-hello` is where the code was
compiled (translation #14).

What `native` buys is the absence of a process boundary: no spawn, no framing,
no serialisation per call. What it costs is the sandbox — a compiled-in extension
shares the address space, so it is capability-*withholding* rather than
contained. `wasm` is the one runtime with a real boundary. See
[`../../README.md`](../../README.md) for the threat model in full.

## Tests

```
cargo test -p orrery-harness --test parity
```

The comparison lives in the facade because `orrery-harness` is the one core crate
`cargo xtask deps-check` allows to name an `extensions/` crate.

The TinyGo leg needs `tinygo` and `wit-bindgen-go`, which cargo cannot install,
so it is behind `--features tinygo-examples`. Without the feature the test prints
that the Go half was **not compared** rather than passing quietly.

Implementation plan:
[`harness/docs/plans/18-writing-an-extension.md`](../../../docs/plans/18-writing-an-extension.md)
(Task 4).
