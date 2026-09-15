import { computed, inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { LibSource, LibSrcStatus } from "../models";
import { UiStore } from "../ui/ui.store";

/**
 * Library sources (M4): the JDK `src.zip` and, per registered project, the
 * crates its `Cargo.lock` pins / the jars its `pom.xml` names — automatic
 * and invisible except for the footer index chip and the dev console. The
 * list is seeded from `libsrc_sources`; `libsrc://status` carries ONE
 * source's run (progress every ~300 ms, then done/error/cancelled) and is
 * merged into the matching row — the backend never re-pushes the whole list.
 *
 * Subscribe-then-seed, like the other stores: a status that lands before
 * the seed answers is kept and folded into the seed row when it arrives.
 */
@Injectable({ providedIn: "root" })
export class LibSrcStore {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);

  readonly sources = signal<LibSource[]>([]);
  /** Source ids with a `libsrc_*` command in flight (buttons disable meanwhile). */
  readonly busy = signal<ReadonlySet<string>>(new Set());
  /** The last `libsrc_sources` rejected (backend without the M4 commands, …). */
  readonly error = signal<string | null>(null);
  readonly seeded = signal(false);

  readonly indexing = computed(() => this.sources().filter((s) => s.state === "indexing"));
  readonly any = computed(() => this.indexing().length > 0);
  /** Files parsed / to parse across every indexing source. */
  readonly done = computed(() => this.indexing().reduce((a, s) => a + (s.done ?? 0), 0));
  readonly total = computed(() => this.indexing().reduce((a, s) => a + (s.total ?? 0), 0));

  /** Progress that arrived before the seed, by source id. */
  private early = new Map<string, LibSrcStatus>();
  private unsubs: (() => void)[] = [];
  /** Roots `ensure()` already asked about this session. */
  private readonly ensured = new Set<string>();

  constructor() {
    this.connect();
  }

  /** Subscribe to `libsrc://status`, then seed. Re-entrant (the e2e specs
   *  stub the bridge after boot and call this to attach to the stub). */
  connect(): void {
    for (const off of this.unsubs) off();
    this.unsubs = [];
    void this.bridge
      .on<LibSrcStatus>(Events.LibSrcStatus, (p) => this.onStatus(p))
      .then((off) => this.unsubs.push(off))
      .catch(() => {});
    void this.refresh();
  }

  /** Re-read the list (`libsrc_sources`); progress already in hand for a
   *  source still indexing survives the replace. */
  async refresh(): Promise<void> {
    try {
      const list = await this.bridge.invoke<LibSource[]>(Commands.LibSrcSources, {});
      this.seed(Array.isArray(list) ? list : []);
      this.error.set(null);
    } catch (e) {
      this.error.set(messageOf(e));
    } finally {
      this.seeded.set(true);
    }
  }

  private seed(list: LibSource[]): void {
    const cur = new Map(this.sources().map((s) => [s.id, s]));
    const next = list.map((s) => {
      const had = cur.get(s.id);
      const early = this.early.get(s.id);
      if (early) return applyStatus(s, early);
      if (s.state === "indexing" && had?.state === "indexing") return { ...s, done: had.done, total: had.total };
      return s;
    });
    this.early.clear();
    this.sources.set(next);
  }

  onStatus(p: LibSrcStatus): void {
    if (!p || !p.sourceId) return;
    const have = this.sources().some((s) => s.id === p.sourceId);
    if (!have) {
      // before the seed (or a source the seed does not know yet): remember
      // it for the seed, and — while it indexes — list a stub row so the
      // footer chip and the modal already show the run
      if (!this.seeded()) this.early.set(p.sourceId, p);
      if (p.state !== "indexing") return;
      this.sources.update((list) => [...list, applyStatus(stubSource(p), p)]);
      return;
    }
    this.sources.update((list) => list.map((s) => (s.id === p.sourceId ? applyStatus(s, p) : s)));
  }

  /** Have the backend discover + index what root `id` needs (idempotent
   *  there; once per root here). Errors are swallowed — an older backend
   *  without the M4 commands must not flash on every editor open. */
  ensure(id: string): void {
    if (!id || this.ensured.has(id)) return;
    this.ensured.add(id);
    void this.bridge.invoke(Commands.LibSrcEnsure, { id }).catch(() => this.ensured.delete(id));
  }

  // ---- row actions (progress and the final state come back on libsrc://status) ----
  async reindex(sourceId: string): Promise<void> {
    // optimistic: the row flips to indexing now; the first status overwrites
    this.sources.update((list) =>
      list.map((s) => (s.id === sourceId ? { ...s, state: "indexing" as const, done: 0, total: 0, error: null } : s)),
    );
    await this.act(sourceId, Commands.LibSrcReindex, "re-index failed");
  }
  async cancel(sourceId: string): Promise<void> {
    await this.act(sourceId, Commands.LibSrcCancel, "cancel failed");
  }
  /** Dev console "Re-scan projects": discovery from scratch on the backend,
   *  every project's sources ensured, then the list re-read. Busy under the
   *  `*` key meanwhile. */
  async rescan(): Promise<void> {
    if (this.busy().has("*")) return;
    this.mark("*", true);
    try {
      const list = await this.bridge.invoke<LibSource[]>(Commands.LibSrcRescan, {});
      this.seed(Array.isArray(list) ? list : []);
      this.error.set(null);
    } catch (e) {
      this.ui.flash(`re-scan failed: ${messageOf(e)}`);
    } finally {
      this.mark("*", false);
    }
  }
  async remove(sourceId: string): Promise<void> {
    if (await this.act(sourceId, Commands.LibSrcRemove, "remove failed")) {
      this.sources.update((list) => list.filter((s) => s.id !== sourceId));
    }
  }

  /** True when the command went through. */
  private async act(sourceId: string, cmd: string, label: string): Promise<boolean> {
    if (this.busy().has(sourceId)) return false;
    this.mark(sourceId, true);
    try {
      await this.bridge.invoke(cmd, { sourceId });
      return true;
    } catch (e) {
      this.ui.flash(`${label}: ${messageOf(e)}`);
      return false;
    } finally {
      this.mark(sourceId, false);
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

/** A status folded into its row: state + progress, decls as they grow, the
 *  error text, and the timestamp once a run completes. */
function applyStatus(s: LibSource, p: LibSrcStatus): LibSource {
  const next: LibSource = { ...s, state: p.state, done: p.done, total: p.total, decls: p.decls, error: p.error ?? null };
  if (p.label) next.label = p.label;
  if (p.kind) next.kind = p.kind;
  if (p.state === "done") {
    next.files = p.total || s.files;
    next.indexedAt = Date.now();
  }
  return next;
}

/** A row for a source only known through its status (pre-seed). */
function stubSource(p: LibSrcStatus): LibSource {
  return {
    id: p.sourceId,
    kind: p.kind,
    path: "",
    label: p.label || p.sourceId,
    state: p.state,
    files: 0,
    decls: 0,
    indexedAt: null,
    sizeBytes: 0,
    projectId: null,
    projectName: null,
    artifacts: 0,
    missing: 0,
    skipped: 0,
    error: null,
  };
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
