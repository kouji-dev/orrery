# wasm-hello-go

The same tool as [`../wasm-hello-rs`](../wasm-hello-rs), in TinyGo, against the
raw `harness/wit/orrery-extension.wit` with `wit-bindgen-go` and **no SDK of
ours**.

That is the whole point of this example. The Rust one proves `orrery-guest`
works; this one proves the *world* works — that a language Orrery ships nothing
for can bind against it and produce a surface the kernel accepts. If writing
this is painful, the WIT is wrong.

## What it needs

Neither is a Rust dependency, and neither is installed by `cargo`:

- [TinyGo](https://tinygo.org) 0.34 or later (`tinygo version`)
- [`wit-bindgen-go`](https://github.com/bytecodealliance/go-modules)
  (`go install go.bytecodealliance.org/cmd/wit-bindgen-go@latest`)

## Build

```sh
wit-bindgen-go generate --world orrery-extension --out internal ../../../wit
tinygo build -target=wasip2 -o wasm-hello-go.wasm \
    --wit-package ../../../wit --wit-world orrery-extension .
```

## In CI, and in `orrery-host-wasm`'s tests

The `examples::both_load_and_dispatch` test builds both examples, loads each and
asserts they produce **identical** output. The Go half is behind the
`tinygo-examples` cargo feature, because a machine without TinyGo cannot build
it and a test that silently skipped would be worse than one that is switched
off on purpose:

```sh
cargo test -p orrery-host-wasm --features tinygo-examples --test examples
```

Without the feature the test still runs, still builds the Rust example, and
prints that the Go half was not compared.
