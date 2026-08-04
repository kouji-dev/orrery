import { effect, inject, Injectable, signal } from "@angular/core";
import { InterestMode } from "../data-source/bridge";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { deriveInterest, paneLeafViews, VisibleSurfaces } from "./interest";

/**
 * A0.2 — publishes the interest set to the backend (`runtime_subscribe`),
 * recomputed whenever the visible surfaces change: tab switch, pane layout
 * change, overview card visibility (viewport scroll), window blur/focus.
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

  // Window focus — blurred demotes stream to digest. document.hasFocus() seeds
  // the right value on a reload that happens while the window is background.
  private windowFocused = signal(
    typeof document === "undefined" ? true : document.hasFocus(),
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
    if (typeof window !== "undefined") {
      window.addEventListener("focus", () => this.windowFocused.set(true));
      window.addEventListener("blur", () => this.windowFocused.set(false));
    }
    effect(() => {
      const entries = deriveInterest(this.surfaces());
      const key = JSON.stringify(entries);
      if (key === this.lastSent) return;
      this.lastSent = key;
      // A1.2 hook: an agent that just left `stream` stops receiving live
      // chunks — its terminal must replay the snapshot when next shown. An
      // agent RE-ENTERING stream while still mounted (blur→focus cycle never
      // re-attaches) recovers immediately instead.
      const next = new Map(entries.map((e) => [e.id, e.mode] as const));
      for (const [id, mode] of this.lastModes) {
        if (mode === "stream" && next.get(id) !== "stream") {
          this.terminals.markStale(id);
        }
      }
      for (const [id, mode] of next) {
        if (mode === "stream" && this.lastModes.get(id) !== "stream") {
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
      // Pane leaves only count when an agent tab is the ACTIVE one — a
      // background tab's terminals are not on screen.
      paneAgents: kind === "agent" && root ? paneLeafViews(root) : [],
      // Mini-preview cards only exist on the orchestrator (overview) tab.
      overviewAgentIds:
        kind === "orchestrator" ? [...this.visibleCards()] : [],
      windowFocused: this.windowFocused(),
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
