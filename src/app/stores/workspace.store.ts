import { effect, inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { GitView, Tab } from "../models";
import { UiStore, WorkspaceLayout } from "../ui/ui.store";
import { PaneNode } from "../workspace/pane-model";
import { ScrollSnapshot, ScrollStateService } from "../workspace/scroll-state.service";

/** Trailing debounce on `workspace_set` — scroll saves fire on every scroll
 *  stop, so writes must coalesce. Small enough that a hard kill loses at most
 *  half a second of scroll position. */
export const WORKSPACE_SAVE_DEBOUNCE_MS = 500;

/** The persisted document. The backend is a passthrough (`serde_json::Value`)
 *  — THIS type is the schema, and it only ever grows per-key so older docs
 *  hydrate cleanly. */
export interface WorkspaceDoc {
  v: 2;
  tabs: Tab[];
  activeTab: string;
  scopeAgentId: string | null;
  paneRoots: Record<string, PaneNode>;
  gitViews: Record<string, GitView | null>;
  diffSelections: Record<string, string | null>;
  diffListWidth: number | null;
  scroll: ScrollSnapshot;
  /** One-shot: agents with a live terminal when "Install & relaunch" ran. */
  updateResume: { at: number; resume: string[] } | null;
}

// localStorage keys: the pre-SQLite home of the layout (migrated on first
// backend load, and still the store when no backend exists — plain `ng serve`).
const LS_KEY = "orrery.workspace";
const LEGACY_RESUME_KEY = "orrery:update-restore";
/** An update-resume list older than this is a failed install that never relaunched. */
const RESUME_MAX_AGE_MS = 15 * 60_000;

/**
 * Owner of the persisted workspace document: what was OPEN — tabs, pane trees,
 * per-agent git views and diff selections, every scroll/view position — stored
 * as one JSON row in the backend SQLite DB (`workspace` table), so the exact
 * workspace survives quits, crashes, updates, and reinstalls that keep app
 * data. `ready()` loads + hydrates BEFORE the shell renders (awaited by the
 * splash screen); a debounced effect persists every change after that.
 *
 * Backend-free fallback (plain `ng serve`): the same document round-trips
 * through localStorage instead, so dev persistence keeps working.
 */
@Injectable({ providedIn: "root" })
export class WorkspaceStore {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly scroll = inject(ScrollStateService);

  /** Gate for the persist effect — nothing writes until hydration applied. */
  private readonly loaded = signal(false);
  /** True when the backend is unreachable — persistence falls back to localStorage. */
  private fallback = false;
  private readonly updateResume = signal<{ at: number; resume: string[] } | null>(null);
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly loadPromise: Promise<void>;

  constructor() {
    this.loadPromise = this.load();
    // Persist on any change: layout signals, per-agent records, scroll rev.
    // Gated on `loaded` (a signal, so the effect re-fires once hydration lands)
    // — an early write would clobber the stored doc with the defaults.
    effect(() => {
      if (!this.loaded()) return;
      const doc = this.currentDoc();
      if (this.saveTimer) clearTimeout(this.saveTimer);
      this.saveTimer = setTimeout(() => {
        this.saveTimer = null;
        void this.write(doc);
      }, WORKSPACE_SAVE_DEBOUNCE_MS);
    });
  }

  /** Resolves once the persisted workspace is loaded AND applied. The splash
   *  screen awaits this before routing into the shell. */
  ready(): Promise<void> {
    return this.loadPromise;
  }

  /** Record the agents to CONTINUE after the update relaunch (null clears).
   *  Callers flush() right after — the installer exits the process. */
  setUpdateResume(resume: string[] | null): void {
    this.updateResume.set(resume ? { at: Date.now(), resume } : null);
  }

  /** Write the current document NOW (bypassing the debounce) — for the moments
   *  the process may die next: update install. */
  async flush(): Promise<void> {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    if (this.loaded()) await this.write(this.currentDoc());
  }

  // ---- load / hydrate ----

  private async load(): Promise<void> {
    let doc: Partial<WorkspaceDoc> | null = null;
    try {
      doc = await this.bridge.invoke<Partial<WorkspaceDoc> | null>(Commands.WorkspaceGet);
      // First backend load on an install that used localStorage: seed from it,
      // then clear the webview copies — SQLite is the source of truth now.
      if (doc == null) {
        doc = this.readLocal();
        try {
          localStorage.removeItem(LS_KEY);
          localStorage.removeItem(LEGACY_RESUME_KEY);
        } catch {
          /* ignore */
        }
      }
    } catch {
      // backend unavailable (plain ng serve) — localStorage carries the doc
      this.fallback = true;
      doc = this.readLocal();
    }
    this.hydrate(doc);
    // `loaded` flips the persist effect on; its first run normalizes whatever
    // was loaded (v1 seed, drained resume) into a fresh v2 write.
    this.loaded.set(true);
  }

  /** The localStorage document: v2 (fallback mode's own writes) or the legacy
   *  v1 layout + separate update-resume key (pre-SQLite installs). */
  private readLocal(): Partial<WorkspaceDoc> | null {
    try {
      const raw = JSON.parse(localStorage.getItem(LS_KEY) || "null");
      const legacy = JSON.parse(localStorage.getItem(LEGACY_RESUME_KEY) || "null");
      // the legacy resume key is one-shot: consumed into the doc right here
      localStorage.removeItem(LEGACY_RESUME_KEY);
      const resume =
        legacy && Array.isArray(legacy.resume)
          ? { at: Number(legacy.at) || 0, resume: legacy.resume as string[] }
          : null;
      if (raw?.v === 2) return { ...raw, updateResume: raw.updateResume ?? resume };
      if (raw?.v === 1) {
        const { tabs, activeTab, scopeAgentId, paneRoots } = raw;
        return { tabs, activeTab, scopeAgentId, paneRoots, updateResume: resume };
      }
      return resume ? { updateResume: resume } : null;
    } catch {
      return null;
    }
  }

  private hydrate(doc: Partial<WorkspaceDoc> | null): void {
    if (!doc) return;
    this.ui.restoreWorkspace(doc as WorkspaceLayout);
    this.scroll.hydrate(doc.scroll);
    // one-shot drain: a fresh resume list continues those terminals this
    // launch; stale or absent → nothing. Either way it does not persist back.
    const r = doc.updateResume;
    if (r && Array.isArray(r.resume) && Date.now() - r.at <= RESUME_MAX_AGE_MS) {
      this.ui.updateResumeIds = r.resume;
    }
    this.updateResume.set(null);
  }

  // ---- persist ----

  private currentDoc(): WorkspaceDoc {
    return {
      v: 2,
      tabs: this.ui.tabs(),
      activeTab: this.ui.activeTab(),
      scopeAgentId: this.ui.scopeAgentId(),
      paneRoots: this.ui.paneRoots(),
      gitViews: this.ui.gitViews(),
      diffSelections: this.ui.diffSelections(),
      diffListWidth: this.ui.diffListWidth(),
      scroll: (this.scroll.rev(), this.scroll.snapshot()),
      updateResume: this.updateResume(),
    };
  }

  private async write(doc: WorkspaceDoc): Promise<void> {
    if (this.fallback) {
      try {
        localStorage.setItem(LS_KEY, JSON.stringify(doc));
      } catch {
        /* storage unavailable — ignore */
      }
      return;
    }
    try {
      await this.bridge.invoke(Commands.WorkspaceSet, { doc });
    } catch {
      // transient backend failure — the next change retries via the effect
    }
  }
}
