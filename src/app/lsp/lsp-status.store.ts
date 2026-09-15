import { computed, inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { LspServer, LspStatus } from "../models";
import { UiStore } from "../ui/ui.store";

/** Rows of one project in the popover / modal (design LspPopover groups). */
export interface LspProjectGroup {
  projectId: string;
  projectName: string;
  rows: LspServer[];
}

/** The chip's one aggregate tone (design `aggLive`): error beats starting
 *  beats running; idle only when EVERY instance idles. */
export type LspAggregate = "starting" | "running" | "idle" | "error" | "none";

/** Instances that still have a process (or are getting one). */
export function isLive(s: LspServer): boolean {
  return s.state !== "stopped" && s.state !== "missing";
}

/** Instances a document should be synced to: up, or about to be. */
export function isSyncable(s: LspServer): boolean {
  return s.state === "starting" || s.state === "ready" || s.state === "idle";
}

/**
 * Language-server instances (M3): the backend's `lsp://status` snapshot
 * mirrored into a signal. Nothing is merged client-side — every push is the
 * WHOLE list, so a missed event can never leave a stale row. Subscribe first,
 * then seed from `lsp_status` (a push that lands during the seed wins —
 * MetricsStore ordering).
 */
@Injectable({ providedIn: "root" })
export class LspStatusStore {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly extensions = inject(ExtensionsStore);

  readonly servers = signal<LspServer[]>([]);
  /** Instance ids with a stop/restart in flight (buttons disable meanwhile). */
  readonly busy = signal<ReadonlySet<string>>(new Set());

  readonly running = computed(() => this.servers().filter(isLive));
  readonly any = computed(() => this.running().length > 0);
  readonly totalMem = computed(() => this.running().reduce((n, s) => n + (s.memBytes || 0), 0));
  readonly starting = computed(() => this.running().filter((s) => s.state === "starting"));
  readonly errors = computed(() => this.running().filter((s) => s.state === "crashed"));
  readonly allIdle = computed(() => this.any() && this.running().every((s) => s.state === "idle"));
  readonly aggregate = computed<LspAggregate>(() => {
    if (this.errors().length) return "error";
    if (this.starting().length) return "starting";
    // "none" is a real footer state, not a reason to hide: the marker is
    // permanent so a crash or a server that never came up is always visible.
    if (!this.any()) return "none";
    if (this.allIdle()) return "idle";
    return "running";
  });
  /** Live instances grouped by project, in first-seen order. */
  readonly byProject = computed<LspProjectGroup[]>(() => {
    const groups: LspProjectGroup[] = [];
    for (const s of this.running()) {
      let g = groups.find((x) => x.projectId === s.projectId);
      if (!g) groups.push((g = { projectId: s.projectId, projectName: s.projectName, rows: [] }));
      g.rows.push(s);
    }
    return groups;
  });

  private unsubs: (() => void)[] = [];
  /** Pushes seen since the last connect — a seed never clobbers one. */
  private pushes = 0;

  constructor() {
    this.connect();
  }

  /** Subscribe to `lsp://status`, then seed. Re-entrant: the e2e specs stub
   *  the bridge after boot and call this to attach to the stub. */
  connect(): void {
    for (const off of this.unsubs) off();
    this.unsubs = [];
    this.pushes = 0;
    void this.bridge
      .on<LspStatus>(Events.LspStatus, (p) => this.onStatus(p))
      .then((off) => this.unsubs.push(off))
      .catch(() => {})
      .then(() => this.seed());
  }

  private async seed(): Promise<void> {
    const before = this.pushes;
    try {
      const st = await this.bridge.invoke<LspStatus>(Commands.LspStatus, {});
      if (this.pushes === before) this.apply(st);
    } catch {
      // optional command — the footer stays empty until the first push
    }
  }

  onStatus(p: LspStatus): void {
    this.pushes++;
    this.apply(p);
  }

  private apply(p: LspStatus | null | undefined): void {
    this.servers.set(Array.isArray(p?.servers) ? p!.servers : []);
  }

  /** Live instances of one server pack (the Extensions modal's sub-rows). */
  instancesOf(extId: string): LspServer[] {
    return this.running().filter((s) => s.extId === extId);
  }

  /** The syncable instance answering `lang` for `projectId`, if any. The
   *  pack's language list counts too: a TypeScript server instance reports
   *  one `language`, but answers for every tag of its pack. */
  liveFor(projectId: string, lang: string): LspServer | undefined {
    if (!lang) return undefined;
    return this.running().find(
      (s) => s.projectId === projectId && isSyncable(s) && (s.language === lang || this.extensions.languagesOfServer(s.extId).includes(lang)),
    );
  }

  /** What the NavHint calls the server that is starting for `lang` in
   *  `projectId` — the instance's label, else the pack name, else generic. */
  labelFor(projectId: string, lang: string): string {
    const inst =
      this.servers().find((s) => s.projectId === projectId && (s.language === lang || this.extensions.languagesOfServer(s.extId).includes(lang))) ??
      this.servers().find((s) => s.language === lang);
    if (inst) return inst.label;
    const pack = this.extensions.servers().find((p) => p.languages.includes(lang));
    return pack?.name ?? "language server";
  }

  // ---- actions (the backend answers on lsp://status) ----
  stop(s: LspServer): Promise<void> {
    return this.act(s.id, Commands.LspStop, { extId: s.extId, projectId: s.projectId }, "stop failed");
  }
  restart(s: LspServer): Promise<void> {
    return this.act(s.id, Commands.LspRestart, { extId: s.extId, projectId: s.projectId }, "restart failed");
  }
  stopAll(): Promise<void> {
    return this.act("*", Commands.LspStopAll, {}, "stop all failed");
  }
  /** Stop every instance of one pack (the modal's per-row "Stop all"). */
  async stopPack(extId: string): Promise<void> {
    await Promise.all(this.instancesOf(extId).map((s) => this.stop(s)));
  }

  private async act(id: string, cmd: string, payload: Record<string, unknown>, label: string): Promise<void> {
    if (this.busy().has(id)) return;
    this.mark(id, true);
    try {
      await this.bridge.invoke(cmd, payload);
    } catch (e) {
      this.ui.flash(`${label}: ${e instanceof Error ? e.message : String(e)}`);
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
