# `@orrery/ext`

Write an Orrery extension in Node. The host starts `node index.mjs`, speaks
JSON-RPC over its stdio, and this package is the other half of that
conversation.

```js
import { defineExtension } from "@orrery/ext";

export default defineExtension({
  tools: {
    impacted: {
      description: "Modules impacted by the current diff",
      input: {
        type: "object",
        properties: { since: { type: "string", description: "A git revision." } },
      },
      requires: ["spawn"],
      async run({ since = "HEAD~1" }, ctx) {
        const out = await ctx.proc.run("java", ["-jar", "bg.jar", "--since", since], {
          timeoutMs: 30_000,
        });
        return ctx.ui.table({
          columns: ["module", "reason"],
          rows: parse(out.stdout),
        });
      },
    },
  },
});
```

`defineExtension` both declares the extension **and** serves it: the host runs
your entry point and expects it to answer, so there is nothing else to call.
Pass `serve: false` (or set `ORRERY_EXT_NO_SERVE=1`) to get the definition back
without starting, which is how a test calls a tool directly.

## The threat model — read this one

**Stripping `fs` is a convenience, not a guarantee.**

`@orrery/ext` ships a loader hook that makes `fs`, `fs/promises` and
`child_process` fail to resolve, with an error naming the brokered call to use
instead. An extension asks for it in its own manifest, because the host starts a
bare `node index.mjs`:

```toml
[process]
command = "node"
args    = ["--import", "@orrery/ext/strip", "index.mjs"]
```

That it is opt-in is the honest shape for it. The hook exists to catch honest
mistakes and to make the brokered path the path of least resistance. It is
**not** a sandbox, and it must never be described as one:

- A Node extension runs as a child process of the harness, with **the harness's
  own OS privileges**. It can read whatever you can read and run whatever you
  can run.
- A determined extension gets around the hook trivially — a native addon,
  `process.binding`, a subprocess of its own, a `node` started without the
  `--import`. The hook stops mistakes, not intent.
- What a child process *does* buy is real: file descriptors and the harness's
  memory are genuinely out of reach, and a crashed guest degrades its extension
  instead of ending your session.
- The capability model is what constrains an extension that *does* go through
  the broker: `ctx.fs`, `ctx.proc`, `ctx.net` and `ctx.creds` are policy-checked
  per call, budgeted, cancellable and audited. Nothing that goes around them is.

**The wasm runtime (plan 14) is the one with a real boundary.** If you are
choosing a runtime because you do not trust the code you are running, choose
that one. If you are choosing Node because the extension is yours and you want
npm, you are choosing convenience — which is a fine thing to choose knowingly.

The same caveat is written into the host's own documentation
(`orrery-host-rpc`'s module docs) and the security docs. Three places, on
purpose: a caveat that lives in one file is a caveat somebody will not read.

## What a tool is given

`run(input, ctx)` gets the validated input and a context that is entirely
brokered. Nothing in it is a handle — there is no `open`, no `Child`, no socket
— because a handle, once given, cannot be budgeted, cancelled or audited.

| | |
|---|---|
| `ctx.fs.list(path, { recursive, limit })` | What is under a directory. A path this call may not read is **not named**. |
| `ctx.fs.read(path, { limit, offset })` | At most `limit` bytes. There is no "read the whole file". |
| `ctx.fs.write(path, text, { atomic })` | All-or-nothing by default; reverted if the call is cancelled. |
| `ctx.proc.run(program, args, { cwd, timeoutMs })` | Contained, under the call's wall clock, output capped. |
| `ctx.net.fetch(url, { method, headers, body })` | One HTTP request, policy-checked. |
| `ctx.creds.get(name)` | A named credential. |
| `ctx.ui.table / .text / .markdown` | **Describes** a surface; never draws one. |
| `ctx.signal` | An `AbortSignal` that fires on `$/cancel`. One call, not the connection. |
| `ctx.budget` | `{ wall_clock_ms, output_bytes, memory_bytes }`. |

A denial comes back as a `BrokerDenied` — `catch` it if the tool has something
better to do than fail, and let it through otherwise: the SDK turns it into the
`denied` outcome every client already renders, rather than a crash.

## What a tool returns

Whatever is easiest, in this order:

- a **surface** from `ctx.ui.*` → an `ok` outcome showing it;
- a plain value → an `ok` outcome carrying it (plus the last surface described);
- an **outcome** built by hand with `outcome.ok / .failed / .denied / .truncated`
  when you want to say exactly which it is;
- a **throw** → `failed`, unless the call was cancelled, in which case
  `cancelled`.

## Testing without a harness

```js
import { defineExtension } from "@orrery/ext";

const ext = defineExtension({ serve: false, tools: { /* … */ } });
const answer = await ext.tools.impacted.run({ since: "HEAD~2" }, fakeCtx);
```

For the real thing — the real ledger, the real denials — use `orrery ext test`,
which loads the manifest and runs the extension against the mock broker the
harness itself ships. No model, no network.

## Also see

- `harness/docs/plans/06-extension-host.md` — the contract this implements.
- `harness/docs/plans/18-writing-an-extension.md` — the walk-through.
- `harness/extensions/examples/node-hello/` — a worked example.
