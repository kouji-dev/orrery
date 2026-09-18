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
 * Phase 4 scope is this registry and the fallback path. Actually *loading* a
 * third party's bundle needs the signed registry, a `render` grant (§4.8) and a
 * sandbox, and a deny rule on that grant degrades every custom surface
 * everywhere — which is the behaviour the fallback already gives.
 *
 * TODO(plan-15): load a renderer from a signed extension bundle under a
 * `render` grant, record the load in the ledger, and refuse an unsigned one.
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
