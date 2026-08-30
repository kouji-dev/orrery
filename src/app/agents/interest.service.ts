import { effect, inject, Injectable, signal } from "@angular/core";
import { InterestMode } from "../data-source/bridge";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { deriveInterest, paneLeafViews, VisibleSurfaces } from "./interest";

/**
 * A0.2 — publishes the interest set to the backend (`runtime_subscribe`),
 * recomputed whenever the visible surfaces change: tab switch, pane layout
 * change, overview card visibility (viewport scroll), window hidden/shown.
 * Replaces the old `agent_focus` single-agent fast path.
 *
 * Also drives the A1.2 recovery hook: when an agent leaves `stream` mode its
 * terminal is marked stale, so the next attach replays the backend scrollback
 * snapshot instead of resuming mid-stream with a gap.
 *
 * Started once from the shell (like FileDropService) — `start()` is a no-op
 * beyond forcing construction; all work lives in the constructor effect.
 */
@Injectable({ providedIn: "root" })
export class InterestService {
  private ui = inject(UiStore);
  private agents = inject(AgentsStore);
  private terminals = inject(TerminalService);

  // Window visibility — hidden (minimized/occluded) demotes stream to digest.
  // Deliberately NOT document focus: on Windows the webview document only
  // gains focus once the user clicks INSIDE the page (native window focus is
  // not forwarded), so a focus-based signal started false and froze every
  // open terminal until the first click. visibilityState seeds correctly at
  // load and `visibilitychange` fires reliably on minimize/restore.
  private windowVisible = signal(
    typeof document === "undefined" ? true : document.visibilityState !== "hidden",
  );

  // Agents whose overview mini-preview card is in the viewport, maintained by
  // the cards' IntersectionObserver registrations (observeCard).
  private visibleCards = signal<ReadonlySet<string>>(new Set());
  private io: IntersectionObserver | null = null;
  private ioAgents = new Map<Element, string>();

  /** Last successfully-sent set (JSON) — dedups the effect's recomputes. */
  private lastSent = "";
  private lastModes = new Map<string, InterestMode>();

  constructor() {
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", () =>
        this.windowVisible.set(document.visibilityState !== "hidden"),
      );
    }
    effect(() => {
      const entries = deriveInterest(this.surfaces());
      const key = JSON.stringify(entries);
      if (key === this.lastSent) return;
      this.lastSent = key;
      // A1.2 hook: any agent NOT in `stream` receives no live chunks — its
      // terminal must replay the snapshot when next shown. Marking every
      // non-stream entry (not just stream→X transitions) also covers agents
      // that were never streamed at all (e.g. first observed as digest), whose
      // gap the old edge-only marking silently dropped. An agent ENTERING
      // stream while still mounted (hidden→shown cycle never re-attaches)
      // recovers immediately instead.
      const next = new Map(entries.map((e) => [e.id, e.mode] as const));
      for (const [id, mode] of this.lastModes) {
        if (mode === "stream" && next.get(id) !== "stream") {
          this.terminals.markStale(id);
        }
      }
      for (const [id, mode] of next) {
        if (mode !== "stream") {
          this.terminals.markStale(id);
        } else if (this.lastModes.get(id) !== "stream") {
          this.terminals.recoverIfStale(id);
        }
      }
      this.lastModes = next;
      void this.agents.subscribe(entries).catch(() => {
        // invoke failed (backend restarting / plain ng serve) — clear the
        // dedup key so the next surface change retries.
        this.lastSent = "";
      });
    });
  }

  /** No-op beyond forcing DI construction (called once from the shell). */
  start(): void {}

  private surfaces(): VisibleSurfaces {
    const tab = this.ui.activeTab();
    const kind = this.ui.activeTabKind();
    const root = this.ui.paneRoots()[tab];
    return {
      // Pane leaves only count when an agent (or v2 project) tab is the ACTIVE
      // one — a background tab's terminals are not on screen. Project tabs ride
      // the same path: their shell PTY is keyed by the project id.
      paneAgents: (kind === "agent" || kind === "project") && root ? paneLeafViews(root) : [],
      // Mini-preview cards only exist on the orchestrator (overview) tab.
      overviewAgentIds:
        kind === "orchestrator" ? [...this.visibleCards()] : [],
      windowVisible: this.windowVisible(),
    };
  }

  /**
   * Register an overview mini-preview card for viewport tracking. Returns the
   * unregister fn (call on component destroy). While the card intersects the
   * viewport its agent is part of the digest set; scrolled-out cards drop to
   * none. Without IntersectionObserver support the card counts as visible from
   * registration (safe fallback — digest is tiny).
   */
  observeCard(el: Element, agentId: string): () => void {
    if (typeof IntersectionObserver === "undefined") {
      this.setCardVisible(agentId, true);
      return () => this.setCardVisible(agentId, false);
    }
    this.io ??= new IntersectionObserver((entries) => {
      for (const e of entries) {
        const id = this.ioAgents.get(e.target);
        if (id) this.setCardVisible(id, e.isIntersecting);
      }
    });
    this.ioAgents.set(el, agentId);
    this.io.observe(el);
    return () => {
      this.io?.unobserve(el);
      this.ioAgents.delete(el);
      this.setCardVisible(agentId, false);
    };
  }

  private setCardVisible(agentId: string, visible: boolean) {
    this.visibleCards.update((prev) => {
      if (visible === prev.has(agentId)) return prev;
      const next = new Set(prev);
      if (visible) next.add(agentId);
      else next.delete(agentId);
      return next;
    });
  }
}
