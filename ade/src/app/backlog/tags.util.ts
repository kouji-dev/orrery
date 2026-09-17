// ── Ticket tags — snake_case helpers shared by the tag chip / picker / filter ──

/** Normalize any free text to a snake_case tag name (max 32 chars). */
export function snakeTag(s: string): string {
  return (s || "")
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 32);
}

/** The sorted union of every tag used across a list of tickets. */
export function allTagsOf(tickets: { tags?: string[] }[]): string[] {
  const set = new Set<string>();
  (tickets || []).forEach((t) => (t.tags || []).forEach((tag) => set.add(tag)));
  return [...set].sort();
}
