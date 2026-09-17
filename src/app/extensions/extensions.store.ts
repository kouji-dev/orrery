import { computed, inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { ExtPack, ExtProgressPayload, ExtRegistryView } from "../models";
import { SettingsStore } from "../settings/settings.store";
import { UiStore } from "../ui/ui.store";

export type ExtSection = "grammars" | "servers" | "updates";

/** Footer state of the registry line: fresh · fetching · last fetch failed. */
export type ExtRegistryState = "ok" | "refreshing" | "offline";

/** An installed pack whose registry version moved past the one on disk. */
export function isExtUpdate(p: ExtPack): boolean {
  return p.installed && p.installedVersion !== null && p.installedVersion !== p.version;
}

/**
 * Extensions: the registry view (grammar packs + language servers + the
 * runtime packs servers ship with), the per-pack download progress, and the
 * Extensions modal's open/section flags.
 *
 * The backend owns every state transition — each `ext_*` command answers with
 * nothing and pushes the WHOLE view again on `ext://status`, which replaces
 * this store's copy (no client-side merging, so a missed event can never
 * leave a stale row). `ext://progress` is the only per-pack stream and is
 * dropped for a pack as soon as a status says it is no longer downloading.
 */
@Injectable({ providedIn: "root" })
export class ExtensionsStore {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly settings = inject(SettingsStore);

  // ---- modal shell state ----
  readonly open = signal(false);
  readonly section = signal<ExtSection>("grammars");

  // ---- registry ----
  readonly view = signal<ExtRegistryView | null>(null);
  readonly packs = computed<ExtPack[]>(() => this.view()?.items ?? []);
  /** Per-pack download progress, keyed by the REQUESTED pack id — a server's
   *  runtime dependency streams under the server's id (`dependency` set). */
  readonly progress = signal<Record<string, ExtProgressPayload>>({});
  /** Pack ids with an `ext_*` command in flight (buttons disable meanwhile). */
  readonly busy = signal<ReadonlySet<string>>(new Set());
  /** A `{refresh:true}` list is in flight. */
  readonly refreshing = signal(false);
  /** The last list/refresh rejected (backend unreachable, registry down…). */
  readonly error = signal<string | null>(null);

  readonly grammars = computed(() => this.packs().filter((p) => p.kind === "grammar"));
  readonly servers = computed(() => this.packs().filter((p) => p.kind === "server"));
  /** Dependency packs (Node, Java) — auto-managed: servers pull them in. */
  readonly runtimes = computed(() => this.packs().filter((p) => p.kind === "runtime"));
  /** Runtime updates count too — a stale Node is a stale server. */
  readonly updates = computed(() => this.packs().filter(isExtUpdate));
  readonly updateCount = computed(() => this.updates().length);
  /** Languages a server pack answers for (`[]` for an unknown id) — the doc
   *  sync and the NavHint gate on it together with `LspServer.language`. */
  languagesOfServer(id: string): string[] {
    return this.packs().find((p) => p.id === id)?.languages ?? [];
  }
  /** Display name of a pack id (the id itself when unknown) — the progress
   *  bar names the dependency being fetched. */
  nameOf(id: string): string {
    return this.packs().find((p) => p.id === id)?.name ?? id;
  }
  /** Names of the INSTALLED packs that list `runtimeId` in `requires` — while
   *  non-empty the runtime's Uninstall is locked (the backend rejects it too). */
  requiredBy(runtimeId: string): string[] {
    return this.packs()
      .filter((p) => p.installed && (p.requires ?? []).includes(runtimeId))
      .map((p) => p.name);
  }
  readonly restartPending = computed(() => this.packs().some((p) => p.state === "pendingRestart"));
  /** The top-bar dot: updates available OR a restart pending. */
  readonly dot = computed(() => this.updateCount() > 0 || this.restartPending());
  readonly registryState = computed<ExtRegistryState>(() => {
    if (this.refreshing()) return "refreshing";
    if (this.view()?.offline || this.error() !== null) return "offline";
    return "ok";
  });

  private unsubs: (() => void)[] = [];

  constructor() {
    this.connect();
  }

  /**
   * Subscribe to the two `ext://` feeds, then seed from the cached registry
   * (subscribe-then-seed: a status that lands between the two still wins).
   * Re-entrant: a second call drops the earlier subscriptions first — the e2e
   * specs stub the bridge after boot and call this to attach to the stub.
   */
  connect(): void {
    for (const off of this.unsubs) off();
    this.unsubs = [];
    void this.bridge
      .on<ExtRegistryView>(Events.ExtStatus, (v) => this.onStatus(v))
      .then((off) => this.unsubs.push(off))
      .catch(() => {});
    void this.bridge
      .on<ExtProgressPayload>(Events.ExtProgress, (p) => this.onProgress(p))
      .then((off) => this.unsubs.push(off))
      .catch(() => {});
    void this.load(false);
  }

  private onStatus(v: ExtRegistryView): void {
    this.view.set(v);
    this.error.set(null);
    // progress only means something for a pack that is still downloading
    const downloading = new Set(v.items.filter((p) => p.state === "downloading").map((p) => p.id));
    this.progress.update((cur) => {
      const next: Record<string, ExtProgressPayload> = {};
      for (const [id, p] of Object.entries(cur)) if (downloading.has(id)) next[id] = p;
      return next;
    });
  }

  /** A tick lands on the requested pack's row, and — while it fetches a
   *  dependency — on the dependency's own row too, as a plain one-step
   *  stream (the backend marks that row `downloading` for the duration). */
  private onProgress(p: ExtProgressPayload): void {
    this.progress.update((cur) => {
      const next = { ...cur, [p.id]: p };
      if (p.dependency) next[p.dependency] = { ...p, id: p.dependency, dependency: null, stepIndex: 1, stepCount: 1 };
      return next;
    });
  }

  private async load(refresh: boolean): Promise<void> {
    if (refresh) {
      if (this.refreshing()) return;
      this.refreshing.set(true);
    }
    try {
      const v = await this.bridge.invoke<ExtRegistryView>(Commands.ExtRegistryList, { refresh });
      this.onStatus(v);
    } catch (e) {
      this.error.set(messageOf(e));
    } finally {
      if (refresh) this.refreshing.set(false);
    }
  }

  /** "Refresh" / "Retry" in the footer: re-fetch the registry index. */
  refresh(): Promise<void> {
    return this.load(true);
  }

  // ---- modal open/close ----
  openModal(section: ExtSection = "grammars"): void {
    this.section.set(section);
    this.open.set(true);
  }
  closeModal(): void {
    this.open.set(false);
  }

  // ---- pack actions (the backend answers on ext://status) ----
  install(id: string): Promise<void> {
    return this.act(id, Commands.ExtInstall, { id }, "install failed");
  }
  uninstall(id: string): Promise<void> {
    return this.act(id, Commands.ExtUninstall, { id }, "uninstall failed");
  }
  setEnabled(id: string, enabled: boolean): Promise<void> {
    return this.act(id, Commands.ExtSetEnabled, { id, enabled }, enabled ? "enable failed" : "disable failed");
  }
  setPath(id: string, path: string | null): Promise<void> {
    return this.act(id, Commands.ExtSetPath, { id, path }, "setting the path failed");
  }
  /** "Locate…": native file picker → manual executable path for a server. */
  async locate(id: string): Promise<void> {
    const current = this.packs().find((p) => p.id === id)?.detection?.path ?? undefined;
    const picked = await this.bridge.pickFile(current);
    if (picked) await this.setPath(id, picked);
  }
  /** "Restart now" on a pack that activates on the next launch. */
  restartNow(): Promise<void> {
    return this.settings.relaunch();
  }

  private async act(id: string, cmd: string, payload: Record<string, unknown>, label: string): Promise<void> {
    if (this.busy().has(id)) return;
    this.mark(id, true);
    try {
      await this.bridge.invoke(cmd, payload);
    } catch (e) {
      this.ui.flash(`${label}: ${messageOf(e)}`);
    } finally {
      this.mark(id, false);
    }
  }

  private mark(id: string, on: boolean): void {
    this.busy.update((s) => {
      const next = new Set(s);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  }
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
