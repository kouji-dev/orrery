# orrery-guest

The guest SDK for Orrery **wasm** extensions. Thin: it hides the arena, names
the broker, and writes the export.

```rust
use orrery_guest::{Ctx, ui};

orrery_guest::export_extension! {
    "impacted" => |input: &str, ctx: &Ctx| -> Result<ui::Node, String> {
        let out = ctx.proc.run("java", &["-jar", "bg.jar"])
            .map_err(|e| e.to_string())?;
        Ok(ui::table(&["module", "reason"], &parse(&out.stdout)))
    },
}
```

You never type an arena index. `ui::*` builds a normal tree and the SDK
flattens it on the way out.

## The three things worth knowing

**A denial is a value.** Every `Ctx` method returns `Result<_, orrery_guest::Error>`
and `Error::Denied` means a policy rule said no. It is terminal — report it and
re-plan. Do not retry it; the SDK will not either.

**You never hold a capability token.** There is nowhere to put one: the type is
not serialisable, so it cannot cross the boundary. Every call asks the host, and
the host looks the call's authority up on its own side.

**Cancellation is coarse.** A cancelled turn traps your component and discards
its store. You get **no chance to clean up**. Anything that must survive has to
have gone through the broker, whose atomic writes are the part that is
transactional.

## What the sandbox costs

No threads, no ambient filesystem, no sockets, a memory ceiling and a wall-clock
ceiling. If your extension needs any of those, it wants the process runtime
instead — which has no sandbox of its own beyond what the broker withholds.
That is a real trade, and this SDK does not pretend otherwise.

## Component size

A Rust component is not small. Measured on `extensions/examples`:

| build | size |
|---|---|
| `--release`, default profile, no SDK (`tests/guests/sandbox-probe`, `opt-level = "z"`, `strip`, `panic = "abort"`) | ~109 KiB |
| `--release` with this SDK and the recipe below | **~53 KiB** |

The recipe, which every example uses:

```toml
[profile.release]
opt-level = "z"
lto = true
strip = true
panic = "abort"
codegen-units = 1
```

Then `wasm-opt -Oz --enable-bulk-memory -o out.wasm in.wasm` for roughly another
fifth. `wasm-opt` is not a cargo dependency and is not required to publish.

## Building

```sh
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
```

WASI **p2**. p3 is a later migration; see plan 14.

## What it asks for, and why

**Nothing.** This crate ships no `orrery.toml`: it is the guest-side SDK an
extension is *written with*, not an extension itself. What the guest may do is
whatever the manifest of the component built on top of it asks for, and the host
answers every import against that grant.

## Tests

```
cargo test -p orrery-guest
```

The arena builder is tested here, on the host, with no wasm involved
(`tests/ui.rs`): the author never touches an index, a child always points
forward from its parent however deep the nesting, a `custom` node carries
exactly one fallback, and strings are escaped. That the result is a
component the real host accepts is proved elsewhere, by
`cargo test -p orrery-host-wasm --test examples` and
`cargo test -p orrery-harness --test parity`, both of which build
`extensions/examples/wasm-hello-rs` and run it.
