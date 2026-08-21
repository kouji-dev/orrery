import { computed, inject, Injectable, signal } from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { AutoApprovePolicy, Settings, SettingsEvents, UpdateInfo } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { WorkspaceStore } from "../stores/workspace.store";
import { UiStore } from "../ui/ui.store";

export type SettingsSection = "updates" | "agent" | "keymap" | "perms" | "notif";

export const SOUND_OPTIONS = ["Ping", "Chime", "Pop", "Glass", "Submarine"] as const;

/** Trailing debounce on `settings_set` so a slider drag / fast toggling lands as
 *  one write. Short enough that "changes apply instantly" stays honest. */
export const SETTINGS_SAVE_DEBOUNCE_MS = 300;

/** The three per-tool override maps. An ABSENT key means "the tool's default" —
 *  `setMap` trims a value back out when it equals that default, so the persisted
 *  document only carries real overrides and dirty checks stay exact. */
export type SettingsMapKey = "toolModel" | "toolEffort" | "autoApprove" | "toolPath";

/** Mirror of the backend `Settings::default()` (src-tauri/src/settings/model.rs).
 *  MUST stay in sync — the per-row reset pills compare against this. */
export function settingsDefaults(): Settings {
  return {
    channel: "stable",
    // Startup surfaces a "update available" toast and lets the user choose to
    // install or skip — no silent self-install. Mirror of the backend default.
    updatePolicy: "notify",
    defaultTool: "", // "" = spawn modal keeps its own hardcoded default
    toolModel: {},
    toolEffort: {},
    toolPath: {}, // absent tool = auto-detect on PATH
    branchTemplate: "agent/{name}",
    worktreeRoot: "", // "" = ctor root (app-data/worktrees)
    projectsRoot: "", // "" = picker opens at the OS default
    autoResume: true,
    // Why off: Ctrl+S + dirty state is the chosen save model (2026-08-08
    // decision); autosave is the opt-in alternative, not the default.
    autosave: false,
    keymap: {}, // absent id = the command's default binding
    autoApprove: {}, // absent tool = "off" (the tool's own flow)
    remoteApproval: true,
    osNotifications: true,
    events: { finished: true, question: true, permission: true, error: true },
    sound: true,
    soundName: "Ping",
    volume: 70,
    // Why 0: no budget cap by default — capping silently would surprise; the
    // user opts in. Why 1.0: a dollar is where an accidental click starts to
    // hurt, so confirm above it out of the box. (Mirror of the Rust defaults.)
    budgetCapUsd: 0,
    confirmAboveUsd: 1,
    costRates: {}, // absent model = EstimateService built-in defaults
    telemetryRawTrace: false, // tracing a flood amplifies it — always opt-in
  };
}

/** The implicit per-tool default a map override is measured against. */
export function settingsMapDefault(key: SettingsMapKey, tool: string): string | null {
  if (key === "autoApprove") return "off";
  if (key === "toolPath") return null; // no implicit default — absent = auto-detect
  const meta = AGENT_TOOLS.find((t) => t.id === tool);
  if (!meta) return null;
  if (key === "toolModel") return meta.models[0];
  return meta.effort ? "high" : null; // toolEffort — spawn's hardcoded default
}

/** Effective manual path override for a tool ("" when none — auto-detect). */
export function effectiveToolPath(s: Settings, tool: string): string {
  return s.toolPath[tool] ?? "";
}

/** Effective (override-or-default) model for a tool. */
export function effectiveModel(s: Settings, tool: string): string {
  return s.toolModel[tool] ?? settingsMapDefault("toolModel", tool) ?? "";
}
/** Effective (override-or-default) effort for a tool ("" when unsupported). */
export function effectiveEffort(s: Settings, tool: string): string {
  return s.toolEffort[tool] ?? settingsMapDefault("toolEffort", tool) ?? "";
}
/** Effective (override-or-default) auto-approve policy for a tool. */
export function effectiveApprove(s: Settings, tool: string): AutoApprovePolicy {
  return s.autoApprove[tool] ?? "off";
}

function deepEq(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every((k) => deepEq((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/**
 * App settings: loads the persisted document on init, applies every edit to the
 * signal INSTANTLY and persists via a debounced `settings_set` (whole-document
 * write — the backend stores one JSON row). Also owns the settings modal's
 * open/section flags and the Updates section's check/install state, so the
 * startup updater flow and the modal share one "update known" source of truth.
 */
@Injectable({ providedIn: "root" })
export class SettingsStore {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly agentsStore = inject(AgentsStore);
  private readonly workspace = inject(WorkspaceStore);

  private readonly defaults = settingsDefaults();
  readonly settings = signal<Settings>(settingsDefaults());
  /** Any value differs from the defaults → footer offers "Reset all". */
  readonly anyDirty = computed(() => !deepEq(this.settings(), this.defaults));

  // ---- modal shell state ----
  readonly open = signal(false);
  readonly openSection = signal<SettingsSection>("updates");

  // ---- update check/install state (shared with the startup updater flow) ----
  readonly checking = signal(false);
  readonly installing = signal(false);
  /** Manual-install progress for the update card: download fraction 0..1. */
  readonly installProgress = signal(0);
  /** 'downloading' while bytes stream, 'installing' once the installer takes
   *  over (the process exits moments later on Windows). */
  readonly installPhase = signal<"downloading" | "installing" | null>(null);
  readonly lastCheckedAt = signal<number | null>(null);
  readonly updateInfo = signal<UpdateInfo | null>(null);
  private readonly updateDismissed = signal(false);
  /** An update is KNOWN (nav dot) — survives "Later". */
  readonly updateKnown = computed(() => this.updateInfo() !== null);
  /** The update card's content, or null after "Later" hid it. */
  readonly updateCard = computed(() => (this.updateDismissed() ? null : this.updateInfo()));
  /** The in-app "What's new" / release-notes modal is open. */
  readonly whatsNewOpen = signal(false);

  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** True once the user edited anything — a late `settings_get` must not clobber. */
  private edited = false;
  private readonly loadPromise: Promise<void>;

  constructor() {
    this.loadPromise = this.load();
  }

  /** Resolves once the persisted settings are merged in (or load failed and the
   *  defaults stand) — the startup updater awaits this for policy/channel. */
  async ready(): Promise<Settings> {
    await this.loadPromise;
    return this.settings();
  }

  private async load(): Promise<void> {
    try {
      const stored = await this.bridge.invoke<Partial<Settings>>(Commands.SettingsGet);
      if (!this.edited) this.settings.set(this.merge(stored));
    } catch {
      // backend unavailable (plain `ng serve`) — defaults stand
    }
  }

  /** Merge a (possibly partial / future-versioned) document over the defaults. */
  private merge(p: Partial<Settings> | null | undefined): Settings {
    const d = settingsDefaults();
    if (!p || typeof p !== "object") return d;
    return {
      ...d,
      ...p,
      toolModel: { ...(p.toolModel ?? {}) },
      toolEffort: { ...(p.toolEffort ?? {}) },
      toolPath: { ...(p.toolPath ?? {}) },
      keymap: { ...(p.keymap ?? {}) },
      autoApprove: { ...(p.autoApprove ?? {}) },
      costRates: { ...(p.costRates ?? {}) },
      events: { ...d.events, ...(p.events ?? {}) },
    };
  }

  // ---- edits (instant-apply + debounced persist) ----
  set(patch: Partial<Settings>): void {
    this.settings.update((s) => ({ ...s, ...patch }));
    this.persist();
  }

  /** Set one per-tool override; a value equal to the tool's implicit default (or
   *  null) REMOVES the key, mirroring the backend's "absent = default". */
  setMap(key: SettingsMapKey, tool: string, value: string | null): void {
    this.settings.update((s) => {
      const next: Record<string, string> = { ...s[key] };
      // Absent = the tool's default; an empty string (e.g. cleared toolPath) or a
      // value equal to the default both fall back to absent.
      if (value == null || value.trim() === "" || value === settingsMapDefault(key, tool))
        delete next[tool];
      else next[tool] = value;
      return { ...s, [key]: next } as Settings;
    });
    this.persist();
  }

  /** Set (or clear, with null) one keybinding override (B6.2 keymap). */
  setKeymapEntry(commandId: string, binding: string | null): void {
    this.settings.update((s) => {
      const next = { ...s.keymap };
      if (binding == null || binding.trim() === "") delete next[commandId];
      else next[commandId] = binding;
      return { ...s, keymap: next };
    });
    this.persist();
  }

  setEvent(kind: keyof SettingsEvents, value: boolean): void {
    this.settings.update((s) => ({ ...s, events: { ...s.events, [kind]: value } }));
    this.persist();
  }

  resetAll(): void {
    this.settings.set(settingsDefaults());
    this.persist();
  }

  private persist(): void {
    this.edited = true;
    if (this.saveTimer !== null) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => {
      this.saveTimer = null;
      // Optimistic: the signal is already the truth; a failed write only flashes.
      void this.bridge
        .invoke(Commands.SettingsSet, { settings: this.settings() })
        .catch(() => this.ui.flash("saving settings failed"));
    }, SETTINGS_SAVE_DEBOUNCE_MS);
  }

  /** Flush a pending debounced write NOW. Modal close and update-install must
   *  not lose the trailing edit: the app can quit inside the 300ms window, and
   *  the backend reads the DB (not our signal) at spawn/install time. */
  flush(): void {
    if (this.saveTimer === null) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = null;
    void this.bridge
      .invoke(Commands.SettingsSet, { settings: this.settings() })
      .catch(() => this.ui.flash("saving settings failed"));
  }

  // ---- modal open/close ----
  openModal(section: SettingsSection = "updates"): void {
    this.openSection.set(section);
    this.open.set(true);
  }
  closeModal(): void {
    this.open.set(false);
    this.flush();
  }

  // ---- updates ----
  /** "Check now": channel-aware `update_check`. Tolerates the legacy bare-string
   *  response and a missing command (T1 in flight) — rejection just flashes. */
  async checkNow(): Promise<void> {
    if (this.checking()) return;
    this.checking.set(true);
    try {
      const r = await this.bridge.invoke<UpdateInfo | string | null>(Commands.UpdateCheck, {
        channel: this.settings().channel,
      });
      const info: UpdateInfo | null = typeof r === "string" ? { version: r } : (r ?? null);
      this.updateInfo.set(info);
      if (info) this.updateDismissed.set(false);
      this.lastCheckedAt.set(Date.now());
    } catch {
      this.ui.flash("update check failed");
    } finally {
      this.checking.set(false);
    }
  }

  /** "Install & relaunch" — on Windows a successful install exits the process,
   *  so this only ever "returns" on failure. While it runs, the update card
   *  tracks the download (update://progress) and the installer handoff
   *  (update://phase) so the user sees more than a frozen button. */
  async install(): Promise<void> {
    if (this.installing()) return;
    this.installing.set(true);
    this.installProgress.set(0);
    this.installPhase.set("downloading");
    this.flush(); // the installer may exit the process — persist edits first
    // Tabs reopen by themselves (the WorkspaceStore persists the layout
    // continuously), but the relaunch must also CONTINUE the running terminals'
    // CLI sessions. The graceful exit path resets running flags, so the
    // crash-only InterruptedAgents mechanism can't cover an update restart —
    // record the live agents in the workspace doc (drained one-shot next boot).
    this.workspace.setUpdateResume(
      this.agentsStore
        .all()
        .filter((a) => a.status === "running")
        .map((a) => a.id),
    );
    await this.workspace.flush();
    const subs: (() => void)[] = [];
    try {
      subs.push(
        await this.bridge.on<{ downloaded: number; total: number | null }>(
          Events.UpdateProgress,
          (p) => this.installProgress.set(p.total && p.total > 0 ? p.downloaded / p.total : 0),
        ),
      );
      subs.push(
        await this.bridge.on<string>(Events.UpdatePhase, (phase) => {
          if (phase === "installing") {
            this.installProgress.set(1);
            this.installPhase.set("installing");
          }
        }),
      );
      await this.bridge.invoke(Commands.UpdateInstall, { channel: this.settings().channel });
    } catch {
      this.ui.flash("update install failed");
      this.installing.set(false);
      this.installPhase.set(null);
      // no relaunch happened — don't resume on a later boot
      this.workspace.setUpdateResume(null);
      void this.workspace.flush();
    } finally {
      // On success the process exits before this runs; on failure it unhooks.
      subs.forEach((off) => off());
    }
  }

  /** The startup flow found an update — make it discoverable in the modal. */
  noteUpdate(info: UpdateInfo): void {
    this.updateInfo.set(info);
    this.lastCheckedAt.set(Date.now());
  }

  /** A BACKGROUND re-check settled (see `UpdateWatcherService`): `null` means the
   *  offer is gone — the user installed from elsewhere, or the release was pulled.
   *
   *  Unlike "Check now", this must not undo a "Later": re-arming the toast on
   *  every poll would make the dismissal meaningless. The dismissal is therefore
   *  cleared only when the offered version actually CHANGED — a newer release
   *  earns a fresh prompt, the same one the user already waved off does not. */
  noteBackgroundUpdate(info: UpdateInfo | null): void {
    this.lastCheckedAt.set(Date.now());
    if (!info) {
      this.updateInfo.set(null);
      this.updateDismissed.set(false);
      return;
    }
    if (this.updateInfo()?.version !== info.version) this.updateDismissed.set(false);
    this.updateInfo.set(info);
  }

  /** "Later": hide the card; the nav dot (update KNOWN) stays. */
  dismissUpdate(): void {
    this.updateDismissed.set(true);
  }

  /** Open / close the in-app "What's new" release-notes modal. */
  openWhatsNew(): void {
    this.whatsNewOpen.set(true);
  }
  closeWhatsNew(): void {
    this.whatsNewOpen.set(false);
  }
}
