import { computed, effect, Injectable, signal } from "@angular/core";
import { AGENT_TOOLS, ORG, WORKTREE_ROOT } from "../data";
import { ContextMenuState, MenuItem, Tab, Tweaks, VizMode } from "../models";
import { hexRgb } from "../utils";
import {
  dropAgent,
  firstLeafOf,
  group,
  leaf,
  openFileInLeaf,
  PaneNode,
  PaneView,
  setAgentView,
  treeAgentIds,
} from "../workspace/pane-model";

const TWEAK_DEFAULTS: Tweaks = {
  theme: "dark",
  palette: ["#a855f7", "#22d3ee"],
  density: "regular",
  defaultViz: "grid",
  rightPanel: true,
  motion: true,
};

const TWEAKS_KEY = "orrery.tweaks";
function loadTweaks(): Tweaks {
  try {
    const saved = JSON.parse(localStorage.getItem(TWEAKS_KEY) || "null");
    return saved ? { ...TWEAK_DEFAULTS, ...saved } : { ...TWEAK_DEFAULTS };
  } catch {
    return { ...TWEAK_DEFAULTS };
  }
}

/**
 * Pure UI / shell state: theme + tweaks, workspace tabs, transient modal flags,
 * the context menu, the toast, and the global run toggle. No domain logic — it
 * depends on nothing else, so every other service can safely use it.
 */
@Injectable({ providedIn: "root" })
export class UiStore {
  readonly tweaks = signal<Tweaks>(loadTweaks());
  readonly viz = signal<VizMode>(TWEAK_DEFAULTS.defaultViz);

  readonly tabs = signal<Tab[]>([
    { id: "orchestrator", kind: "orchestrator" },
    { id: "backlog", kind: "backlog" },
  ]);
  readonly activeTab = signal<string>("orchestrator");
  // the "focused" agent within the active tab — drives the sidebar highlight and
  // the right panel scope. A tab can tile several agents; this is the live one.
  readonly scopeAgentId = signal<string | null>(null);
  private tabSeq = 0;
  private newTabId(): string {
    return "tab" + ++this.tabSeq;
  }

  /** Computed kind of the currently active tab (defaults to "orchestrator"). */
  readonly activeTabKind = computed<string>(() => {
    const id = this.activeTab();
    return this.tabs().find((t) => t.id === id)?.kind ?? "orchestrator";
  });

  /** The ticketId of the active tab when its kind is "ticket", else null. */
  readonly activeTicketId = computed<string | null>(() => {
    const id = this.activeTab();
    const tab = this.tabs().find((t) => t.id === id);
    return tab?.kind === "ticket" ? (tab.ticketId ?? null) : null;
  });

  /** Signal for the ticket to dispatch (read by the spawn modal in Phase 7). */
  readonly spawnTicketId = signal<string | null>(null);

  readonly query = signal<string>("");
  readonly toast = signal<string>("");

  readonly spawning = signal<{ project: string | null } | null>(null);
  readonly addingProject = signal<boolean>(false);
  readonly contextMenu = signal<ContextMenuState | null>(null);

  readonly org = ORG;
  readonly worktreeRoot = WORKTREE_ROOT;
  readonly tools = AGENT_TOOLS;

  private toastTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    // reflect tweaks onto <html> + accent css vars
    effect(() => {
      const t = this.tweaks();
      const r = document.documentElement;
      r.setAttribute("data-theme", t.theme);
      r.setAttribute("data-density", t.density);
      r.setAttribute("data-motion", t.motion ? "on" : "off");
      const [a1, a2] = t.palette;
      r.style.setProperty("--accent", a1);
      r.style.setProperty("--accent-2", a2);
      r.style.setProperty("--accent-rgb", hexRgb(a1));
      r.style.setProperty("--accent-2-rgb", hexRgb(a2));
    });
    // keep viz in sync with the default
    effect(() => this.viz.set(this.tweaks().defaultViz));
    // persist tweaks across sessions
    effect(() => {
      try {
        localStorage.setItem(TWEAKS_KEY, JSON.stringify(this.tweaks()));
      } catch {
        /* storage unavailable — ignore */
      }
    });
  }

  // ---- tweaks ----
  setTweak<K extends keyof Tweaks>(key: K, value: Tweaks[K]) {
    this.tweaks.update((t) => ({ ...t, [key]: value }));
  }
  toggleTheme() {
    this.setTweak("theme", this.tweaks().theme === "dark" ? "light" : "dark");
  }

  // ---- toast ----
  flash(msg: string) {
    this.toast.set(msg);
    if (this.toastTimer) clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => this.toast.set(""), 2600);
  }

  // ---- tabs ----
  // A tab id is NOT an agent id: an agent tab owns a pane tree (keyed in
  // `paneRoots`) that may tile several agents. `scopeAgentId` tracks the focused
  // one. Opening an agent reuses any tab already showing it, else makes a new tab.
  openAgent(agentId: string, pane?: string) {
    const view: PaneView = pane === "diff" || pane === "file" ? "diff" : "terminal";
    this.scopeAgentId.set(agentId);
    const roots = this.paneRoots();
    const existing = this.tabs().find(
      (tb) => tb.kind !== "orchestrator" && roots[tb.id] && treeAgentIds(roots[tb.id]).includes(agentId),
    );
    if (existing) {
      this.activeTab.set(existing.id);
      // An EXPLICIT pane request must win in the reused tab too — without this,
      // "Review diff" on a notification silently landed on the agent's terminal
      // whenever its tab already existed. Bare openAgent(id) calls (sidebar,
      // overview) pass no pane and keep whatever view the user had open.
      if (pane !== undefined) {
        this.paneRoots.update((m) => ({ ...m, [existing.id]: setAgentView(m[existing.id], agentId, view) }));
      }
      return;
    }
    const id = this.newTabId();
    this.paneRoots.update((m) => ({ ...m, [id]: leaf(agentId, view) }));
    this.tabs.update((prev) => [...prev, { id, kind: "agent" }]);
    this.activeTab.set(id);
  }
  selectTab(id: string) {
    this.activeTab.set(id);
    const root = this.paneRoots()[id];
    if (root) {
      const ids = treeAgentIds(root);
      if (ids.length) this.scopeAgentId.set(ids[0]);
    }
  }
  setScope(agentId: string) {
    this.scopeAgentId.set(agentId);
  }
  closeTab(id: string) {
    // Backlog tab is a pinned singleton — never close it
    if (id === "backlog") return;
    this.tabs.update((prev) => prev.filter((x) => x.id !== id));
    this.paneRoots.update((m) => {
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
    if (this.activeTab() === id) this.activeTab.set("orchestrator");
  }

  /** Focus the always-present pinned backlog tab. */
  openBacklog() {
    this.activeTab.set("backlog");
  }

  /** Open a ticket tab — reuse if one for this ticketId already exists. */
  openTicket(ticketId: string) {
    const existing = this.tabs().find(
      (t) => t.kind === "ticket" && t.ticketId === ticketId,
    );
    if (existing) {
      this.activeTab.set(existing.id);
      return;
    }
    const id = this.newTabId();
    this.tabs.update((prev) => [...prev, { id, kind: "ticket", ticketId }]);
    this.activeTab.set(id);
  }

  /** Open a draft (new) ticket tab. */
  openTicketDraft() {
    const id = this.newTabId();
    this.tabs.update((prev) => [...prev, { id, kind: "ticket", ticketId: "draft" }]);
    this.activeTab.set(id);
  }

  /** Set spawnTicketId then open the spawn modal for the given project. */
  dispatchTicket(ticket: { id: string; projectId: string | null }) {
    this.spawnTicketId.set(ticket.id);
    this.openSpawn(ticket.projectId);
  }

  // ---- split-pane workspace ----
  // Each agent tab's workspace is a pane tree, keyed here by the tab id.
  readonly paneRoots = signal<Record<string, PaneNode>>({});
  /** Replace (or update via a fn) an agent tab's pane tree. */
  setRoot(tabId: string, root: PaneNode | ((prev: PaneNode) => PaneNode)) {
    this.paneRoots.update((m) => {
      const prev = m[tabId];
      const next = typeof root === "function" ? (prev ? root(prev) : prev) : root;
      return next ? { ...m, [tabId]: next } : m;
    });
  }

  // ---- tab grouping / tiling (driven by tab + pane drag-and-drop) ----
  /** Merge the src tab's tree into the dst tab as a side-by-side tile; drop src. */
  mergeTabs(srcId: string, dstId: string) {
    if (srcId === dstId) return;
    const roots = this.paneRoots();
    const srcRoot = roots[srcId];
    const dstRoot = roots[dstId];
    if (!srcRoot || !dstRoot) return;
    const merged = group(dstRoot, srcRoot);
    this.paneRoots.update((m) => {
      const { [srcId]: _drop, ...rest } = m;
      return { ...rest, [dstId]: merged };
    });
    this.tabs.update((prev) => prev.filter((t) => t.id !== srcId));
    this.activeTab.set(dstId);
    this.flash("grouped into tiled tab");
  }
  /** Add `agentId` as a new tiled terminal pane inside an existing tab. */
  addAgentToTab(agentId: string, tabId: string) {
    const root = this.paneRoots()[tabId];
    this.scopeAgentId.set(agentId);
    if (root && !treeAgentIds(root).includes(agentId)) {
      this.paneRoots.update((m) => ({ ...m, [tabId]: group(m[tabId], leaf(agentId, "terminal")) }));
      this.flash("added terminal to tab");
    }
    this.activeTab.set(tabId);
  }
  /** Reorder a tab in the strip, dropping it before/after the target tab. */
  reorderTab(srcId: string, dstId: string, before: boolean) {
    this.tabs.update((prev) => {
      const src = prev.find((t) => t.id === srcId);
      if (!src) return prev;
      const arr = prev.filter((t) => t.id !== srcId);
      const idx = arr.findIndex((t) => t.id === dstId);
      if (idx < 0) return prev;
      arr.splice(before ? idx : idx + 1, 0, src);
      return arr;
    });
  }
  /** Explode a grouped tab into one solo tab per agent. */
  ungroupTab(tabId: string) {
    const root = this.paneRoots()[tabId];
    if (!root) return;
    const ids = treeAgentIds(root);
    if (ids.length < 2) return;
    const made = ids.map((aid) => ({ id: this.newTabId(), aid }));
    this.tabs.update((prev) => {
      const idx = prev.findIndex((t) => t.id === tabId);
      if (idx < 0) return prev;
      const copy = [...prev];
      copy.splice(idx, 1, ...made.map((m) => ({ id: m.id, kind: "agent" as const })));
      return copy;
    });
    this.paneRoots.update((m) => {
      const { [tabId]: _drop, ...rest } = m;
      made.forEach((mm) => (rest[mm.id] = leaf(mm.aid, "terminal")));
      return { ...rest };
    });
    this.activeTab.set(made[0].id);
    this.flash("ungrouped");
  }
  /** Pull one agent out of a grouped tab into its own new tab beside it. */
  detachAgent(tabId: string, agentId: string) {
    const root = this.paneRoots()[tabId];
    if (!root) return;
    const ids = treeAgentIds(root);
    if (!ids.includes(agentId) || ids.length < 2) return;
    const nt = this.newTabId();
    this.tabs.update((prev) => {
      const idx = prev.findIndex((t) => t.id === tabId);
      if (idx < 0) return prev;
      const copy = [...prev];
      copy.splice(idx + 1, 0, { id: nt, kind: "agent" });
      return copy;
    });
    this.paneRoots.update((m) => ({ ...m, [tabId]: dropAgent(root, agentId), [nt]: leaf(agentId, "terminal") }));
    this.scopeAgentId.set(agentId);
    this.activeTab.set(nt);
  }

  // ---- sidebar compact rail ----
  readonly sidebarCompact = signal(false);
  toggleSidebarCompact() {
    this.sidebarCompact.update((v) => !v);
  }

  // ---- right-panel file tree → open the file as a tab INSIDE the agent's pane ----
  // Each file gets its own closable tab in the pane's file strip; re-opening an
  // already-open file just re-activates its tab.
  openFileInWorkspace(agentId: string, path: string) {
    this.openAgent(agentId); // ensure the agent's tab exists + is active (keeps its view)
    const tabId = this.activeTab();
    this.paneRoots.update((m) => {
      const root = m[tabId];
      if (!root) return m;
      const leafId = firstLeafOf(root, agentId);
      if (!leafId) return m;
      return { ...m, [tabId]: openFileInLeaf(root, leafId, path) };
    });
  }

  // ---- context menu ----
  openMenu(e: MouseEvent, items: MenuItem[]) {
    e.preventDefault();
    e.stopPropagation();
    this.contextMenu.set({ x: e.clientX, y: e.clientY, items });
  }
  closeMenu() {
    this.contextMenu.set(null);
  }

  // ---- modal openers ----
  openSpawn(projectId: string | null) {
    this.spawning.set({ project: projectId });
  }
  closeSpawn() {
    this.spawning.set(null);
    this.clearSpawnTicket();
  }
  /** Clear the pending dispatch ticket (called once by the spawn modal on init,
   *  and automatically by closeSpawn so stale links don't leak). */
  clearSpawnTicket() {
    this.spawnTicketId.set(null);
  }
  openAddProject() {
    this.addingProject.set(true);
  }
  closeAddProject() {
    this.addingProject.set(false);
  }
}
