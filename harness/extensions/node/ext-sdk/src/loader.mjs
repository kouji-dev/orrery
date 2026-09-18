// The loader hook that strips `fs` and `child_process` from an extension.
//
// **This is a convenience, not a boundary.** It catches the honest mistake and
// makes the brokered path the path of least resistance. It is not a guarantee:
// the child runs with the harness's OS privileges, and a determined extension
// has `process.binding`, a native addon, or simply a `node` started without
// this hook. The README says so at length, and so does the host's own module
// documentation. Only wasm (plan 14) has a real boundary.
//
// Usage — whoever starts node installs it. The host's default convention is a
// bare `node index.mjs`, so an extension that wants the hook asks for it in its
// own manifest:
//
//     [process]
//     command = "node"
//     args    = ["--import", "@orrery/ext/strip", "index.mjs"]
//
// That it is opt-in is the honest shape for it: a hook the host imposed would
// look like a boundary the host enforces, and it is not one.

/** Modules an extension is asked to reach through the broker instead. */
export const STRIPPED = new Map([
  ["fs", "ctx.fs.read / ctx.fs.write / ctx.fs.list"],
  ["node:fs", "ctx.fs.read / ctx.fs.write / ctx.fs.list"],
  ["fs/promises", "ctx.fs.read / ctx.fs.write / ctx.fs.list"],
  ["node:fs/promises", "ctx.fs.read / ctx.fs.write / ctx.fs.list"],
  ["child_process", "ctx.proc.run"],
  ["node:child_process", "ctx.proc.run"],
]);

/** Node's module-resolution hook. */
export async function resolve(specifier, context, next) {
  const instead = STRIPPED.get(specifier);
  if (instead !== undefined) {
    throw Object.assign(
      new Error(
        `\`${specifier}\` is not available to an Orrery extension: use ${instead}, ` +
          `which is policy-checked, budgeted and auditable. ` +
          `(This hook is a convenience, not a sandbox — see @orrery/ext's README.)`,
      ),
      { code: "ERR_MODULE_NOT_FOUND" },
    );
  }
  return next(specifier, context);
}
