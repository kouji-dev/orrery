/**
 * Viewport-aware placement for the shared menu chrome — pure so the flip /
 * clamp rules are unit-testable without a DOM.
 *
 * Everything is in viewport (client) coordinates: `.menu-panel` is
 * position:fixed, so its `left` / `top` / `bottom` ARE these numbers.
 */

/** The point (or control edge) the menu hangs off. */
export interface MenuAnchor {
  /** Anchor x. With `alignX: 'left'` this is the panel's left edge; with
   *  `'right'` it is its right edge (a dropdown pinned to a control's right). */
  x: number;
  /** Anchor y — the panel's top edge before any flip. */
  y: number;
  alignX: "left" | "right";
  /** Dropup anchor: the anchor element's TOP. When the panel would overflow
   *  the viewport bottom, its BOTTOM pins here instead of flipping at the
   *  point. null = no anchor element (a bare click point). */
  flipY: number | null;
}

export interface MenuSize {
  w: number;
  h: number;
}

export interface Viewport {
  w: number;
  h: number;
}

export interface MenuPlacement {
  x: number;
  y: number;
  /** Non-null = dropup: pinned via CSS `bottom`, so late content growth keeps
   *  the panel's bottom edge on the anchor instead of pushing it off-screen. */
  bottom: number | null;
}

/** Breathing room kept between the panel and every viewport edge. */
export const MENU_GAP = 8;

/**
 * Place a menu of `size` at `anchor` inside `vp`.
 *
 * - overflows the RIGHT edge → flip to the other side of the anchor (the
 *   panel's right edge lands on the anchor); clamp only if that side is too
 *   narrow as well;
 * - overflows the BOTTOM → pin the bottom to `flipY` when the caller gave an
 *   anchor element, else flip above the point; clamp if neither fits;
 * - never leaves the viewport: the result is clamped to `MENU_GAP` on every
 *   side it can still honour.
 *
 * A panel taller/wider than the viewport is capped by CSS (`max-height` /
 * `max-width` on `.menu-panel`), so `size` is already viewport-sized here and
 * the clamp always lands at `MENU_GAP`.
 */
export function placeMenu(
  anchor: MenuAnchor,
  size: MenuSize,
  vp: Viewport,
  gap: number = MENU_GAP,
): MenuPlacement {
  const maxX = vp.w - size.w - gap;
  const maxY = vp.h - size.h - gap;

  // ---- horizontal ----
  let x = anchor.alignX === "right" ? anchor.x - size.w : anchor.x;
  if (x > maxX) {
    // flip across the anchor first — a clamped menu hides the row it was
    // opened from, a flipped one does not. The flip is itself clamped: an
    // anchor sitting INSIDE the gutter would otherwise push the flipped panel
    // back past the edge.
    const flipped = anchor.alignX === "right" ? anchor.x : anchor.x - size.w;
    x = flipped >= gap ? Math.min(flipped, maxX) : maxX;
  }
  if (x < gap) x = gap;

  // ---- vertical ----
  let y = anchor.y;
  let bottom: number | null = null;
  if (y > maxY) {
    const fy = anchor.flipY;
    if (fy != null && size.h <= fy - gap) {
      bottom = vp.h - fy;
    } else {
      const up = anchor.y - size.h;
      y = up >= gap ? Math.min(up, maxY) : maxY;
    }
  }
  if (bottom == null && y < gap) y = gap;

  return { x, y, bottom };
}
