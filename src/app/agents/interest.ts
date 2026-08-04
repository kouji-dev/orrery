import { InterestEntry, InterestMode } from "../data-source/bridge";
import { PaneNode, PaneView } from "../workspace/pane-model";

/**
 * A0.2 — pure derivation of the backend interest set from what is VISIBLE.
 *
 * Rules (renderer bytes are proportional to what is on screen, flat in total
 * agent count):
 *  • an agent shown in a TERMINAL pane of the active tab → `stream`
 *    (demoted to `digest` only while the window is HIDDEN — minimized or
 *    fully occluded. NOT on mere focus loss: an unfocused-but-visible window
 *    is exactly how agents are watched from a second monitor, and webview
 *    document focus lags native window focus on Windows, which froze open
 *    terminals until the user clicked into them);
 *  • an agent whose overview mini-preview card is in the viewport → `digest`;
 *  • an agent shown only in a diff/file pane → none (PTY output is irrelevant
 *    to that surface — status still arrives over hooks);
 *  • anything unmounted → none (absent from the returned set).
 *
 * When one agent has several surfaces the strongest mode wins
 * (stream > digest > none). `none` is expressed by ABSENCE — the returned
 * entries only ever carry stream/digest, which keeps the wire payload
 * proportional to visible surfaces.
 */
export interface VisibleSurfaces {
  /** The active tab's pane leaves: which agent each shows, and as what view. */
  paneAgents: { agentId: string; view: PaneView }[];
  /** Agents whose overview mini-preview card is currently in the viewport. */
  overviewAgentIds: string[];
  /** Window visibility — a HIDDEN (minimized/occluded) window demotes stream
   *  to digest. Focus is deliberately not consulted: see the module doc. */
  windowVisible: boolean;
}

const RANK: Record<InterestMode, number> = { none: 0, digest: 1, stream: 2 };

export function deriveInterest(s: VisibleSurfaces): InterestEntry[] {
  const modes = new Map<string, InterestMode>();
  const bump = (id: string, mode: InterestMode) => {
    const cur = modes.get(id) ?? "none";
    if (RANK[mode] > RANK[cur]) modes.set(id, mode);
    else if (!modes.has(id)) modes.set(id, cur);
  };
  for (const p of s.paneAgents) {
    if (p.view === "terminal") {
      bump(p.agentId, s.windowVisible ? "stream" : "digest");
    } else {
      bump(p.agentId, "none"); // diff/file view open — PTY output irrelevant
    }
  }
  for (const id of s.overviewAgentIds) bump(id, "digest");
  return [...modes.entries()]
    .filter(([, mode]) => mode !== "none")
    .map(([id, mode]) => ({ id, mode }))
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)); // stable → cheap set-equality
}

/** Flatten a pane tree into its (agentId, view) leaves — the `paneAgents`
 *  input for {@link deriveInterest}. Unassigned leaves are skipped. */
export function paneLeafViews(root: PaneNode): { agentId: string; view: PaneView }[] {
  const out: { agentId: string; view: PaneView }[] = [];
  const walk = (n: PaneNode) => {
    if (n.type === "leaf") {
      if (n.agentId) out.push({ agentId: n.agentId, view: n.view });
    } else {
      walk(n.a);
      walk(n.b);
    }
  };
  walk(root);
  return out;
}
