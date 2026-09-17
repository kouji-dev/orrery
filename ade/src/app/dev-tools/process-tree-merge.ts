import { ProcessNode, ProcessTreeSnapshot } from "../models";

/** Expand key of the synthetic root row (kept out of any real `rootId:pid`
 *  namespace so user toggles on real nodes can never collide with it). */
export const MERGED_ROOT_KEY = "orrery:root";

/**
 * Fold the backend's per-root forest (the "app" root + one root per agent —
 * carved apart there for attribution) into the single tree the Resources tab
 * renders: a synthetic "Orrery App" root whose children are the app process
 * subtree followed by every agent subtree, and whose numbers are the recursive
 * totals of everything Orrery runs. Backend subtree rollups already exclude
 * external-browser (`excluded`) nodes, so the sums stay honest.
 */
export function mergeRoots(t: ProcessTreeSnapshot): ProcessNode | null {
  if (!t.roots.length) return null;
  const app = t.roots.find((r) => r.id === "app");
  const agents = t.roots.filter((r) => r !== app);
  const sum = (pick: (n: ProcessNode) => number) => t.roots.reduce((a, r) => a + pick(r.node), 0);
  return {
    pid: 0, // not a real process — the row renders "—"
    name: "Orrery App",
    note: "rust core + webview + agents",
    // own = subtree: the root IS the total, per-column and in the rollup.
    cpu: sum((n) => n.subtreeCpu),
    privBytes: sum((n) => n.subtreePrivBytes),
    rssBytes: 0,
    subtreeCpu: sum((n) => n.subtreeCpu),
    subtreePrivBytes: sum((n) => n.subtreePrivBytes),
    subtreeProcs: sum((n) => n.subtreeProcs),
    detached: false,
    excluded: false,
    children: [...(app ? [app.node] : []), ...agents.map((r) => r.node)],
  };
}
