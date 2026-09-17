/**
 * Blame presentation helpers, shared by the two surfaces that show it:
 * the standalone Annotate view (diff pane) and the Monaco blame gutter
 * (file pane). Pure — no DOM, no Monaco — so both can be unit tested.
 */

/** Stable per-author hash. */
function hashOf(author: string): number {
  let hash = 0;
  for (let i = 0; i < author.length; i++) hash = ((hash * 31) + author.charCodeAt(i)) >>> 0;
  return hash;
}

/** Stable per-author hue — mirrors authorColor in code-diff.component.ts. */
export function authorColor(author: string): string {
  return `hsl(${((hashOf(author) % 300) + 30) % 360}, 60%, 66%)`;
}

/** How many hue buckets the Monaco gutter has classes for. */
export const BLAME_HUES = 12;

/** Bucket an author into one of BLAME_HUES hues. The gutter colours through a
 *  CSS class (Monaco injected text takes a class name, not a style), so the
 *  continuous hue of `authorColor` has to collapse to a fixed set. */
export function authorBucket(author: string): number {
  return Math.floor((((hashOf(author) % 300) + 30) % 360) / (360 / BLAME_HUES));
}

/** 1–2 uppercase initials from a display name. */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join("");
}

/** Unix seconds → short relative string, e.g. "2h", "3d", "5m". Empty for 0. */
export function relTime(when: number, now = Date.now()): string {
  if (!when) return "";
  const secs = Math.floor(now / 1000) - when;
  if (secs < 0) return "now";
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  const days = Math.floor(hrs / 24);
  if (days < 30) return `${days}d`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;
  return `${Math.floor(months / 12)}y`;
}

/**
 * Age background using the same color-mix formula from the design reference.
 * age 0 = newest → most opaque ink; age 1 = oldest → most transparent.
 */
export function ageBg(age: number): string {
  return `color-mix(in oklch, var(--ui-ink), transparent ${86 + Math.round(age * 11)}%)`;
}

/** Column widths of the Monaco gutter label, in monospace characters. */
const W_INITIALS = 2;
const W_REL = 4;
const W_SHA = 7;

/** Total label width — every line gets exactly this many characters so the code
 *  starts at one column, whether or not the line opens a commit run. */
export const BLAME_LABEL_WIDTH = W_INITIALS + 1 + W_REL + 1 + W_SHA + 1;

/**
 * One line's gutter label: `AB   2d a1b2c3d `, or all spaces for a line that
 * continues the commit above it (the design's run-grouping, kept in a column
 * that never reflows). Truncated/padded to BLAME_LABEL_WIDTH exactly.
 */
export function blameLabel(
  row: { author: string; sha: string; when: number },
  first: boolean,
  now = Date.now(),
): string {
  if (!first) return " ".repeat(BLAME_LABEL_WIDTH);
  const who = initials(row.author).slice(0, W_INITIALS).padEnd(W_INITIALS);
  const rel = relTime(row.when, now).slice(0, W_REL).padStart(W_REL);
  // an uncommitted line has no sha to show — keep the column, drop the digits
  const sha = (row.sha && !/^0+$/.test(row.sha) ? row.sha.slice(0, W_SHA) : "·".repeat(W_SHA))
    .padEnd(W_SHA);
  return `${who} ${rel} ${sha} `;
}
