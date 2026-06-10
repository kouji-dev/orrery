import { computed, inject, Injectable, signal } from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { BRIDGE, Commands } from "../data-source/bridge";
import { AutoApprovePolicy, Settings, SettingsEvents, UpdateInfo } from "../models";
import { UiStore } from "../ui/ui.store";

export type SettingsSection = "updates" | "agent" | "perms" | "notif";

export const SOUND_OPTIONS = ["Ping", "Chime", "Pop", "Glass", "Submarine"] as const;

/** Trailing debounce on `settings_set` so a slider drag / fast toggling lands as
 *  one write. Short enough that "changes apply instantly" stays honest. */
export const SETTINGS_SAVE_DEBOUNCE_MS = 300;

/** The three per-tool override maps. An ABSENT key means "the tool's default" —
 *  `setMap` trims a value back out when it equals that default, so the persisted
 *  document only carries real overrides and dirty checks stay exact. */
export type SettingsMapKey = "toolModel" | "toolEffort" | "autoApprove";

/** Mirror of the backend `Settings::default()` (src-tauri/src/settings/model.rs).
 *  MUST stay in sync — the per-row reset pills compare against this. */
export function settingsDefaults(): Settings {
  return {
    channel: "stable",
    // Design SETTINGS_DEFAULTS are the contract: notify / resume on / remote on.
    updatePolicy: "notify",
    defaultTool: "", // "" = spawn modal keeps its own hardcoded default
    toolModel: {},
    toolEffort: {},
    branchTemplate: "agent/{name}",
    worktreeRoot: "", // "" = ctor root (app-data/worktrees)
    autoResume: true,
    autoApprove: {}, // absent tool = "off" (the tool's own flow)
    remoteApproval: true,
    osNotifications: true,
    events: { finished: true, question: true, permission: true, error: true },
    sound: true,
    soundName: "Ping",
    volume: 70,
  };
}

/** The implicit per-tool default a map override is measured against. */
export function settingsMapDefault(key: SettingsMapKey, tool: string): string | null {
  if (key === "autoApprove") return "off";
  const meta = AGENT_TOOLS.find((t) => t.id === tool);
  if (!meta) return null;
  if (key === "toolModel") return meta.models[0];
  return meta.effort ? "high" : null; // toolEffort — spawn's hardcoded default
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
  readonly lastCheckedAt = signal<number | null>(null);
  readonly updateInfo = signal<UpdateInfo | null>(null);
  private readonly updateDismissed = signal(false);
  /** An update is KNOWN (nav dot) — survives "Later". */
  readonly updateKnown = computed(() => this.updateInfo() !== null);
  /** The update card's content, or null after "Later" hid it. */
  readonly updateCard = computed(() => (this.updateDismissed() ? null : this.updateInfo()));

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
      autoApprove: { ...(p.autoApprove ?? {}) },
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
      if (value == null || value === settingsMapDefault(key, tool)) delete next[tool];
      else next[tool] = value;
      return { ...s, [key]: next } as Settings;
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

  // ---- modal open/close ----
  openModal(section: SettingsSection = "updates"): void {
    this.openSection.set(section);
    this.open.set(true);
  }
  closeModal(): void {
    this.open.set(false);
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
   *  so this only ever "returns" on failure. */
  async install(): Promise<void> {
    if (this.installing()) return;
    this.installing.set(true);
    try {
      await this.bridge.invoke(Commands.UpdateInstall, { channel: this.settings().channel });
    } catch {
      this.ui.flash("update install failed");
      this.installing.set(false);
    }
  }

  /** The startup flow found an update — make it discoverable in the modal. */
  noteUpdate(info: UpdateInfo): void {
    this.updateInfo.set(info);
    this.lastCheckedAt.set(Date.now());
  }

  /** "Later": hide the card; the nav dot (update KNOWN) stays. */
  dismissUpdate(): void {
    this.updateDismissed.set(true);
  }
}
