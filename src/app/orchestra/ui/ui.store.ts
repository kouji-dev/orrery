import { effect, Injectable, signal } from "@angular/core";
import { AGENT_TOOLS, ORG, WORKTREE_ROOT } from "../data";
import { ContextMenuState, MenuItem, Tab, Tweaks, VizMode } from "../models";
import { hexRgb } from "../utils";

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

  readonly tabs = signal<Tab[]>([{ id: "orchestrator" }]);
  readonly activeTab = signal<string>("orchestrator");
  // pane hint when an agent tab is opened from a deep action
  readonly paneHint: Record<string, string> = {};

  readonly query = signal<string>("");
  readonly running = signal<boolean>(true);
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
  openAgent(id: string, pane?: string) {
    if (pane) this.paneHint[id] = pane;
    this.tabs.update((prev) => (prev.find((x) => x.id === id) ? prev : [...prev, { id }]));
    this.activeTab.set(id);
  }
  selectTab(id: string) {
    this.activeTab.set(id);
  }
  closeTab(id: string) {
    this.tabs.update((prev) => prev.filter((x) => x.id !== id));
    if (this.activeTab() === id) this.activeTab.set("orchestrator");
  }

  // ---- run toggle ----
  toggleRunAll() {
    const wasRunning = this.running();
    this.running.set(!wasRunning);
    this.flash(wasRunning ? "paused all agents" : "resumed all agents");
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
