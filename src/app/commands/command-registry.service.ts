import { computed, inject, Injectable, Signal, signal } from "@angular/core";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { BRIDGE, Commands } from "../data-source/bridge";
import { ProjectActionsService } from "../projects/project-actions.service";
import { SettingsStore } from "../settings/settings.store";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { FileSaveService } from "../workspace/file-save.service";
import { TabCloseGuardService } from "../workspace/tab-close-guard.service";
import { PaneLeaf, PaneNode } from "../workspace/pane-model";
import { ToolWindowStore } from "../tool-window/tool-window.store";
import { EditorNavService } from "./editor-nav.service";
import { matchBinding } from "./fuzzy";
import { RecentFilesService } from "./recent-files.service";

/** One registered app action (roadmap B2.2). Everything the palette, Search
 *  Everywhere and the keybinding dispatcher know comes from this record. */
export interface AppCommand {
  id: string;
  label: string;
  group: string;
  icon: string;
  /** Binding like "Ctrl+Shift+p" ("Mod" = Cmd on mac)"Shift Shift" is special. */
  kbd?: string;
  /** Secondary binding (e.g. the IntelliJ variant of the same action). */
  kbdAlt?: string;
  danger?: boolean;
  /** Enablement — computed at registry-build time from live signals. */
  enabled: boolean;
  run: () => void;
}

export type OverlayKind = "palette" | "search" | "recent" | "goto" | "find";
export interface OverlayState {
  kind: OverlayKind;
  /** Initial Search-Everywhere tab (e.g. "files" for Go to File). */
  tab?: string;
}

/** Double-shift window (ms) for Search Everywhere. */
const DOUBLE_SHIFT_MS = 380;

/**
 * The command registry + global keybinding dispatcher (roadmap B2.2). Every
 * palette-reachable action registers here once; the palette and Search
 * Everywhere render FROM this list, and one window-level capture listener
 * drives every binding (plus double-shift). Start once from the shell.
 */
@Injectable({ providedIn: "root" })
export class CommandRegistryService {
  private ui = inject(UiStore);
  private settings = inject(SettingsStore);
  private runtime = inject(AgentRuntimeService);
  private agentActions = inject(AgentActionsService);
  private work = inject(AgentWorkStore);
  private projects = inject(ProjectActionsService);
  private recents = inject(RecentFilesService);
  private editorNav = inject(EditorNavService);
  private toolWindow = inject(ToolWindowStore);
  private saver = inject(FileSaveService);
  private edits = inject(EditsStore);
  private closeGuard = inject(TabCloseGuardService);

  /** The open overlay (null = none). */
  readonly overlay = signal<OverlayState | null>(null);

  private lastShift = 0;
  private shiftArmed = false;
  private started = false;

  open(kind: OverlayKind, tab?: string): void {
    this.overlay.set({ kind, tab });
  }
  close(): void {
    this.overlay.set(null);
  }

  /** The active pane leaf currently showing a file (drives Go to Line). */
  readonly activeFileLeaf: Signal<PaneLeaf | null> = computed(() => {
    const root = this.ui.paneRoots()[this.ui.activeTab()];
    if (!root) return null;
    const scope = this.ui.scopeAgentId();
    let fallback: PaneLeaf | null = null;
    let scoped: PaneLeaf | null = null;
    const walk = (n: PaneNode): void => {
      if (n.type === "leaf") {
        if (n.view === "file" && n.activeFile && n.agentId) {
          fallback ??= n;
          if (n.agentId === scope) scoped ??= n;
        }
        return;
      }
      walk(n.a);
      walk(n.b);
    };
    walk(root);
    return scoped ?? fallback;
  });

  /** Open `path` in `agentId`'s workspace, optionally jumping to a line. */
  openFileAt(agentId: string, path: string, line?: number, col = 1): void {
    this.ui.openFileInWorkspace(agentId, path);
    this.recents.record(agentId, path);
    if (line && line > 0) this.editorNav.goTo(agentId, path, line, col);
  }

  /** The registry, rebuilt reactively (labels + enablement track live state). */
  readonly commands: Signal<AppCommand[]> = computed(() => {
    const ag = this.runtime.activeAgent();
    const proj = ag ? this.projects.projectOf(ag.projectId) : undefined;
    const branchTarget = proj?.branch ?? "main";
    const hasChanges = !!ag && this.work.changesFor(ag.id).data.length > 0;
    const activeTab = this.ui.activeTab();
    const tabKind = this.ui.activeTabKind();
    const fileLeaf = this.activeFileLeaf();
    const anyRunning = this.agentActions.anyRunning();
    const rightPanel = this.ui.tweaks().rightPanel;
    const theme = this.ui.tweaks().theme;
    const dirtyCount = this.edits.dirtyKeys().size;
    const keymap = this.settings.settings().keymap;

    const c = (
      cmd: Omit<AppCommand, "enabled"> & { enabled?: boolean },
    ): AppCommand => ({ ...cmd, enabled: cmd.enabled ?? true });

    return [
      // ---- navigate ----
      c({ id: "search.everywhere", label: "Search Everywhere", group: "Navigate", icon: "search", kbd: "Shift Shift", kbdAlt: "Ctrl+k", run: () => this.open("search") }),
      c({ id: "palette.open", label: "Show All Commands", group: "Navigate", icon: "bolt", kbd: "Ctrl+Shift+p", kbdAlt: "Ctrl+Shift+a", run: () => this.open("palette") }),
      c({ id: "recent.files", label: "Recent Files", group: "Navigate", icon: "clock", kbd: "Ctrl+e", run: () => this.open("recent") }),
      c({ id: "goto.line", label: "Go to Line…", group: "Navigate", icon: "enter", kbd: "Ctrl+l", enabled: !!fileLeaf, run: () => this.open("goto") }),
      c({ id: "goto.file", label: "Go to File…", group: "Navigate", icon: "file", kbd: "Ctrl+Shift+o", enabled: !!ag, run: () => this.open("search", "files") }),
      c({ id: "view.orchestrator", label: "Open Orchestrator", group: "Navigate", icon: "grid", run: () => this.ui.selectTab("orchestrator") }),
      c({ id: "view.backlog", label: "Open Backlog", group: "Navigate", icon: "archive", run: () => this.ui.openBacklog() }),
      c({
        id: "tab.close", label: "Close Tab", group: "Navigate", icon: "x",
        enabled: tabKind !== "orchestrator" && activeTab !== "backlog",
        run: () => this.closeGuard.requestClose(activeTab),
      }),

      // ---- search ----
      c({ id: "find.inFiles", label: "Find in Files", group: "Search", icon: "search", kbd: "Ctrl+Shift+f", enabled: !!ag || this.projects.all().length > 0, run: () => this.open("find") }),

      // ---- git (scope agent) ----
      c({ id: "git.commit", label: "Commit Changes", group: "Git", icon: "commit", enabled: hasChanges, run: () => ag && this.agentActions.act(ag.id, "commit") }),
      c({ id: "git.push", label: "Push to origin", group: "Git", icon: "push", enabled: !!ag && ag.commits > 0, run: () => ag && this.agentActions.act(ag.id, "push") }),
      c({ id: "git.rebase", label: `Rebase onto ${branchTarget} (AI)`, group: "Git", icon: "sparkles", enabled: !!ag, run: () => ag && this.agentActions.act(ag.id, "rebase") }),
      c({ id: "git.merge", label: `Merge ${branchTarget} (AI)`, group: "Git", icon: "merge", enabled: !!ag, run: () => ag && this.agentActions.act(ag.id, "merge") }),
      c({ id: "git.discard", label: "Discard Changes", group: "Git", icon: "discard", danger: true, enabled: hasChanges, run: () => ag && this.agentActions.act(ag.id, "discard") }),

      // ---- git tool window (bottom dock; bindings from the design's command map) ----
      c({ id: "git.graph", label: "Show Commit Graph", group: "Git", icon: "graph", kbd: "Ctrl+Shift+g", kbdAlt: "Ctrl+9", run: () => this.toolWindow.toggle("graph") }),
      c({ id: "git.branches", label: "Branches & Remotes", group: "Git", icon: "branch", kbd: "Ctrl+Shift+b", run: () => this.toolWindow.toggle("branches") }),
      c({ id: "git.localHistory", label: "Local History", group: "Git", icon: "clock", run: () => this.toolWindow.toggle("history") }),

      // ---- agents ----
      c({ id: "agent.spawn", label: "Spawn Agent…", group: "Agents", icon: "bolt", run: () => this.ui.openSpawn(ag?.projectId ?? this.projects.all()[0]?.id ?? null) }),
      c({
        id: "agent.pauseResume",
        label: ag?.status === "running" ? `Pause ${ag.name}` : `Run ${ag?.name ?? "Agent"}`,
        group: "Agents", icon: ag?.status === "running" ? "pause" : "play",
        enabled: !!ag, run: () => ag && this.agentActions.toggleRun(ag),
      }),
      c({ id: "agent.runAll", label: anyRunning ? "Pause All Agents" : "Run All Agents", group: "Agents", icon: anyRunning ? "pause" : "play", run: () => this.agentActions.toggleRunAll() }),
      c({ id: "agent.terminal", label: "Open Agent Terminal", group: "Agents", icon: "terminal", enabled: !!ag, run: () => ag && this.ui.openAgent(ag.id, "terminal") }),
      c({ id: "agent.diff", label: "Review Agent Diff", group: "Agents", icon: "diff", enabled: !!ag, run: () => ag && this.ui.openAgent(ag.id, "diff") }),
      c({ id: "agent.delete", label: "Delete Worktree", group: "Agents", icon: "trash", danger: true, enabled: !!ag, run: () => ag && this.ui.openDeleteWorktree(ag.id) }),
      c({ id: "ticket.new", label: "New Ticket", group: "Agents", icon: "plus", run: () => this.ui.openTicketDraft() }),
      c({ id: "project.add", label: "Add Project…", group: "Agents", icon: "folderOpen", run: () => this.ui.openAddProject() }),

      // ---- workspace ----
      c({
        id: "file.save", label: "Save All", group: "Workspace", icon: "file", kbd: "Ctrl+s",
        // JetBrains semantics: Ctrl+S writes EVERY dirty buffer. Enabled with a
        // file tab active OR any dirty buffer anywhere — a clean workspace
        // saves as a silent no-op, so Ctrl+S never flashes "unavailable"
        enabled: !!fileLeaf?.activeFile || dirtyCount > 0,
        run: () => void this.saver.saveAll(),
      }),
      c({ id: "ws.rightPanel", label: (rightPanel ? "Hide" : "Show") + " Right Panel", group: "Workspace", icon: "panelLeft", run: () => this.ui.setTweak("rightPanel", !this.ui.tweaks().rightPanel) }),
      c({ id: "ws.sidebar", label: "Toggle Compact Sidebar", group: "Workspace", icon: "columns", run: () => this.ui.toggleSidebarCompact() }),
      c({ id: "ws.theme", label: `Switch to ${theme === "dark" ? "Light" : "Dark"} Theme`, group: "Workspace", icon: theme === "dark" ? "sun" : "moon", run: () => this.ui.toggleTheme() }),

      // ---- tools ----
      c({ id: "settings.open", label: "Settings", group: "Tools", icon: "settings", kbd: "Ctrl+,", run: () => this.settings.openModal() }),
      c({ id: "app.whatsNew", label: "What's New", group: "Tools", icon: "flag", run: () => this.settings.openWhatsNew() }),
    ].map((cmd) => {
      // B6.2 keymap: a user override replaces the default binding (and its alt
      // variant — a stale alt could shadow another command's new binding).
      // Applied HERE so the dispatcher, palette chips, Search Everywhere and
      // the Keymap settings rows all see the same effective binding.
      const kbd = keymap[cmd.id];
      return kbd ? { ...cmd, kbd, kbdAlt: undefined } : cmd;
    });
  });

  /** Install the global key listener (idempotent; call from the shell). */
  start(): void {
    if (this.started) return;
    this.started = true;
    window.addEventListener("keydown", this.onKeydown, true);
  }

  /** B6.2: true while the Keymap settings row is recording a chord — the
   *  dispatcher must not RUN the chord being recorded. */
  readonly captureMode = signal(false);

  private onKeydown = (e: KeyboardEvent): void => {
    if (this.captureMode()) return; // the keymap recorder owns this keystroke
    const target = e.target as HTMLElement | null;
    const tag = target?.tagName ?? "";
    const typing = tag === "INPUT" || tag === "TEXTAREA" || !!target?.isContentEditable;
    // xterm routes keys through a hidden helper textarea — plain-Ctrl chords
    // (Ctrl+E = readline end-of-line, Ctrl+L = clear) must stay with the shell.
    const inTerminal = !!target?.classList?.contains("xterm-helper-textarea");

    // double-shift → Search Everywhere (disabled while typing, like the design)
    if (e.key === "Shift" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      const now = Date.now();
      if (!typing && this.shiftArmed && now - this.lastShift < DOUBLE_SHIFT_MS) {
        this.shiftArmed = false;
        e.preventDefault();
        this.open("search");
        return;
      }
      this.lastShift = now;
      this.shiftArmed = true;
      return;
    }
    this.shiftArmed = false;

    if (e.key === "Escape" && this.overlay()) {
      e.preventDefault();
      e.stopPropagation();
      this.close();
      return;
    }

    for (const cmd of this.commands()) {
      const kbds = [cmd.kbd, cmd.kbdAlt].filter((k): k is string => !!k && k !== "Shift Shift");
      if (!kbds.some((k) => matchBinding(e, k))) continue;
      // while typing, only modifier chords may fire; inside a terminal only
      // Ctrl+Shift chords may (plain Ctrl belongs to the shell/readline)
      if (typing && !(e.ctrlKey || e.metaKey)) return;
      if (inTerminal && !(e.shiftKey && (e.ctrlKey || e.metaKey))) return;
      e.preventDefault();
      e.stopPropagation();
      if (cmd.enabled) cmd.run();
      else this.ui.flash(cmd.label + " — not available here");
      return;
    }
  };
}

/** Cached per-agent worktree file listings (Search Everywhere "Files" corpus).
 *  Short TTL — a worktree walk is cheap but not free at 20k entries. */
@Injectable({ providedIn: "root" })
export class WorkspaceFilesService {
  private bridge = inject(BRIDGE);
  private cache = new Map<string, { at: number; files: string[] }>();
  private inflight = new Map<string, Promise<string[]>>();

  filesFor(agentId: string, ttlMs = 30_000): Promise<string[]> {
    const hit = this.cache.get(agentId);
    if (hit && Date.now() - hit.at < ttlMs) return Promise.resolve(hit.files);
    const running = this.inflight.get(agentId);
    if (running) return running;
    const p = this.bridge
      .invoke<string[]>(Commands.SearchFiles, { id: agentId })
      .then((files) => {
        this.cache.set(agentId, { at: Date.now(), files });
        return files;
      })
      .catch(() => hit?.files ?? [])
      .finally(() => this.inflight.delete(agentId));
    this.inflight.set(agentId, p);
    return p;
  }
}
