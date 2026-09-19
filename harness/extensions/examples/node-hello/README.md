# `node-hello`

The worked example for `@orrery/ext`: two tools, a manifest, and nothing else.

- `greet` describes a surface and returns it.
- `count_files` discovers through `ctx.fs.list`, so a path the policy hides is
  not named in the answer, and declares `requires = ["read"]` so that without a
  `read` grant it is **disabled** with a reason in the ledger rather than
  failing at its first call.

Run it the way the harness does:

```
orrery ext test harness/extensions/examples/node-hello
```

No model, no network, no key. `orrery-host-rpc`'s `sdk` test loads this very
directory through the real `RpcHost` and dispatches both tools, which is what
keeps the example and the SDK honest about the protocol.

No deviation from a published extension: the import is `@orrery/ext`. The SDK
travels inside the `orrery` binary and the host vendors it into
`node_modules/@orrery/ext` beside the extension before starting it — which is
what makes `orrery install ./node-hello` produce something a turn can call,
with no registry and no network. The vendored directory is ignored by git.

**Before you reach for `fs`:** read `@orrery/ext`'s README. The loader hook that
strips it is a convenience, not a boundary.
