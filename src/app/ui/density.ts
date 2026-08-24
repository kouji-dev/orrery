/**
 * Density bridge for the surfaces CSS cannot reach.
 *
 * Monaco and xterm render into a canvas / absolutely-positioned DOM they own,
 * so they take metrics as JS numbers rather than inheriting `font-size`. That
 * makes them the one place where a density switch does NOT propagate for free
 * — everything else in the app is driven by the `--sp-*` / `--fs-*` tokens.
 *
 * Both surfaces read the SAME `--fs-code` / `--lh-code` tokens through here, so
 * the editor and the terminal stay on one metric and a change to the ramp in
 * styles.css moves them together.
 */

/** Code font size + line height for the current density, in px. */
export function codeMetrics(): { fontSize: number; lineHeight: number } {
  const cs = getComputedStyle(document.documentElement);
  const fontSize = parseFloat(cs.getPropertyValue("--fs-code")) || 13;
  const lineHeight = parseFloat(cs.getPropertyValue("--lh-code")) || 18;
  return { fontSize, lineHeight };
}

/** Structural row height (`--row-h`) for the current density, in px. */
export function rowHeight(): number {
  const cs = getComputedStyle(document.documentElement);
  return parseFloat(cs.getPropertyValue("--row-h")) || 30;
}

/**
 * Resolved pixel value of any registered density token (`--sp-9`, `--row-h`,
 * `--ctl-h`, …). Only works for properties registered with `@property` in
 * styles.css — an unregistered custom property has no computed value and
 * getPropertyValue hands back its raw token stream instead of a length.
 *
 * @param name  Custom property name, including the leading `--`.
 * @param fallback  Used when the token is missing or unregistered.
 */
export function tokenPx(name: string, fallback: number): number {
  const v = parseFloat(
    getComputedStyle(document.documentElement).getPropertyValue(name),
  );
  return Number.isFinite(v) ? v : fallback;
}
