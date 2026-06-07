import { effect, Injectable, signal } from "@angular/core";
import { AGENT_TOOLS, ORG, WORKTREE_ROOT } from "../data";
import { ContextMenuState, MenuItem, Tab, Tweaks, VizMode } from "../models";
import { hexRgb } from "../utils";
import { dropAgent, group, leaf, PaneNode, PaneView, treeAgentIds } from "../workspace/pane-model";

const TWEAK_DEFAULTS: Tweaks = {
  theme: "dark",
  palette: ["#a855f7", "#22d3ee"],
  density: "regular",
  defaultViz: "grid",
  rightPanel: true,
  motion: true,
};

const TWEAKS_KEY = "katrix.tweaks";
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

  readonly tabs = signal<Tab[]>([{ id: "orchestrator", kind: "orchestrator" }]);
  readonly activeTab = signal<string>("orchestrator");
  // the "focused" agent within the active tab — drives the sidebar highlight and
  // the right panel scope. A tab can tile several agents; this is the live one.
  readonly scopeAgentId = signal<string | null>(null);
  private tabSeq = 0;
  private newTabId(): string {
    return "tab" + ++this.tabSeq;
  }

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
    this.tabs.update((prev) => prev.filter((x) => x.id !== id));
    this.paneRoots.update((m) => {
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
    if (this.activeTab() === id) this.activeTab.set("orchestrator");
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

  // ---- right-panel file tree → open the agent's diff in the workspace ----
  // (the standalone single-file viewer tab is superseded by the pane tree)
  openFileInWorkspace(agentId: string, _path: string) {
    this.openAgent(agentId, "diff");
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
  }
  openAddProject() {
    this.addingProject.set(true);
  }
  closeAddProject() {
    this.addingProject.set(false);
  }
}
