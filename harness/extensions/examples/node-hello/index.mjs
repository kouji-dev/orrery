// A node extension, whole. `node index.mjs` is all the host does to start it.
//
// A published extension writes `import { defineExtension } from "@orrery/ext"`.
// This one lives in the same repository as the SDK and is not installed from a
// registry, so it reaches for the source directly — the only difference.

import { defineExtension } from "../../node/ext-sdk/src/index.mjs";

export default defineExtension({
  tools: {
    greet: {
      description: "Say hello to someone.",
      input: {
        type: "object",
        properties: { name: { type: "string", description: "Who to greet." } },
        required: ["name"],
      },
      async run({ name }, ctx) {
        return ctx.ui.text(`hello, ${name}`);
      },
    },

    count_files: {
      description: "How many files are under a directory, as the policy sees it.",
      input: {
        type: "object",
        properties: { path: { type: "string" } },
        required: ["path"],
      },
      // Without this, the tool is *disabled* rather than failing at its first
      // call: the ledger says why, and the model is never offered it.
      requires: ["read"],
      async run({ path }, ctx) {
        // Through the broker, so a path this call may not read is not even
        // named — and `ctx.signal` is what a cancellation arrives on.
        const listing = await ctx.fs.list(path, { recursive: true, limit: 1000 });
        const files = listing.entries.filter((e) => !e.is_dir);
        return ctx.ui.table({
          columns: ["what", "count"],
          rows: [
            ["files", files.length],
            ["truncated", String(listing.truncated)],
          ],
        });
      },
    },

    // The fourth writing of one tool. `native-hello`, `wasm-hello-rs` and
    // `wasm-hello-go` return exactly this, and
    // `orrery-harness/tests/parity.rs` asserts the four are equal. It takes no
    // input and asks for no capability on purpose: the comparison is about
    // dispatch, not about policy.
    parity: {
      description:
        "Describe a fixed two-row table. The same tool exists in the native and wasm " +
        "examples and returns exactly this, which is how the harness proves the kernel " +
        "cannot tell the runtimes apart.",
      input: { type: "object", properties: {}, additionalProperties: false },
      async run(_input, ctx) {
        return ctx.ui.table({
          columns: ["key", "value"],
          rows: [
            ["tool", "parity"],
            ["runtime", "irrelevant"],
          ],
        });
      },
    },
  },
});
