/**
 * The custom-renderer registry.
 *
 * The one thing this client can do that the ratatui one will not: draw an
 * extension's `custom` surface with the extension's own React component.
 *
 * ```ts
 * registerRenderer("buildgraph.flamegraph", Flamegraph);
 * ```
 *
 * Phase 4 scope is this registry and the fallback path: a `custom` surface
 * whose kind nobody registered draws its `fallback` subtree, so a missing or
 * refused renderer degrades rather than blanks.
 *
 * # Why a third party's bundle still does not load here
 *
 * Plan 15 landed `orrery-registry` — ed25519 signature verification, a keyring
 * with rotation, and the `install`/`remove` verbs — and `Aspect.Render` exists
 * in `orrery-proto`, so the grant this would run under is real. Two things
 * between that and `import()`ing a stranger's component are still genuinely
 * missing, and neither is work this file can do:
 *
 * 1. **The client has no filesystem contract with the kernel.** Everything this
 *    process gets arrives over `--endpoint` / `$ORRERY_ENDPOINT` as AG-UI
 *    frames — deliberately, because that is the whole contract a third-party
 *    client implements. No frame announces "extension X holds `render` and its
 *    renderer entry is at <path>"; `AguiEvent.Custom` carries a name and a
 *    value and nothing about where code lives. Reaching into
 *    `~/.orrery/extensions/` instead would make this client's contract the
 *    filesystem, which is the contract the endpoint replaced.
 * 2. **Verification would have to happen twice.** `orrery-registry` verifies
 *    with `ring` at install time and writes no receipt beside the unpacked
 *    bundle — `orrery.toml` is the only file it puts there. A loader in this
 *    process therefore has nothing to trust, and re-implementing ed25519 in TS
 *    would make the same trust decision in a second place, which is precisely
 *    what a single signed index exists to prevent.
 *
 * A third, smaller one: `import()` in this process hands the module the
 * client's own Node privileges, so a denied `render` grant would be
 * unenforceable rather than merely degraded.
 *
 * The unblocking change is a host-side one — an install receipt plus a frame
 * (or an endpoint method) that names the verified renderer bundles this session
 * may load. Until that exists, `registerRenderer` is how a renderer gets in:
 * an embedder that compiles the component in has already made the trust
 * decision itself.
 */

import type { ComponentType } from "react";

/** What a custom renderer is handed. */
export interface CustomProps {
  /** `<ext>.<name>`, so two extensions cannot claim one renderer. */
  kind: string;
  /** Whatever that renderer needs. */
  payload: Record<string, unknown>;
  /** The column budget. */
  width: number;
  /** Where an answer goes, if the renderer collects one. */
  onIntent?: (surface: string, value: unknown) => void;
  /** The surface's id, when it has one. */
  id?: string | null;
}

/** A registered renderer. */
export type CustomRenderer = ComponentType<CustomProps>;

const RENDERERS = new Map<string, CustomRenderer>();

/** Register a renderer for one `custom` kind. Last registration wins. */
export function registerRenderer(kind: string, renderer: CustomRenderer): void {
  RENDERERS.set(kind, renderer);
}

/** The renderer for a kind, if anyone registered one. */
export function rendererFor(kind: string): CustomRenderer | undefined {
  return RENDERERS.get(kind);
}

/** Every registered kind, for `orrery ext list` and for tests. */
export function registered(): string[] {
  return [...RENDERERS.keys()].sort();
}

/** Forget every renderer. Tests use it; nothing else should. */
export function clearRenderers(): void {
  RENDERERS.clear();
}
