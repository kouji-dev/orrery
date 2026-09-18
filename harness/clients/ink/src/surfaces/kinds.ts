/**
 * What every surface component is handed.
 *
 * A component is a pure function of these props. None of them reaches the
 * session: an interaction calls `onIntent`, and `<App>` — the only thing
 * holding the session — turns that into an `intent` the kernel validates.
 */

import type { Status, Surface, SurfaceKind } from "@orrery/protocol";

/** A surface as a client holds it, whether it came from the store or from a child. */
export interface Drawable {
  id?: string | null;
  status?: Status | null;
  kind: SurfaceKind;
}

/** One `SurfaceKind` variant, picked by its tag. */
export type KindOf<T extends SurfaceKind["t"]> = Extract<SurfaceKind, { t: T }>;

/** What a person's edit produces. Never a state write. */
export type Intent = (surface: string, value: unknown) => void;

/** The props every surface component takes. */
export interface SurfaceProps<T extends SurfaceKind["t"] = SurfaceKind["t"]> {
  node: Drawable & { kind: KindOf<T> };
  /** The column budget. Components clip; they never assume 80. */
  width: number;
  /** Whether this surface has the keyboard. At most one surface does. */
  active?: boolean;
  /** Where an answer goes. */
  onIntent?: Intent;
}

/** A child of a `stack`, as a `Drawable`. */
export function drawable(surface: Surface): Drawable {
  return { id: surface.id ?? null, status: surface.status ?? null, kind: surface.kind };
}
