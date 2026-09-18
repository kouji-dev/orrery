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

One deviation from a published extension: the import is
`../../node/ext-sdk/src/index.mjs` rather than `@orrery/ext`, because the SDK
lives in this repository and is not installed from a registry here.

**Before you reach for `fs`:** read `@orrery/ext`'s README. The loader hook that
strips it is a convenience, not a boundary.
