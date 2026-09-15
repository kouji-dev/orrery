import { computed, inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Events } from "../data-source/bridge";
import { IndexStatus } from "../models";

/**
 * Symbol-index progress per root, fed by `symbols://index` (M2). Nothing is
 * seeded: the footer chip is transient by design — it appears when a root
 * starts indexing and goes away when it is ready. A root that errors stays
 * listed (tinted) until its next run replaces the entry.
 */
@Injectable({ providedIn: "root" })
export class IndexStatusStore {
  private readonly bridge = inject(BRIDGE);

  /** Latest status per root id. */
  readonly byRoot = signal<Record<string, IndexStatus>>({});

  readonly indexing = computed(() => Object.values(this.byRoot()).filter((s) => s.state === "indexing"));
  readonly errored = computed(() => Object.values(this.byRoot()).filter((s) => s.state === "error"));
  /** Files parsed / to parse across every indexing root. */
  readonly done = computed(() => this.indexing().reduce((a, s) => a + s.done, 0));
  readonly total = computed(() => this.indexing().reduce((a, s) => a + s.total, 0));
  /** The chip shows while something indexes or the last run failed. */
  readonly visible = computed(() => this.indexing().length > 0 || this.errored().length > 0);

  private unsubs: (() => void)[] = [];

  constructor() {
    this.connect();
  }

  /** Subscribe to `symbols://index`. Re-entrant (the e2e specs stub the
   *  bridge after boot and call this to attach to the stub). */
  connect(): void {
    for (const off of this.unsubs) off();
    this.unsubs = [];
    void this.bridge
      .on<IndexStatus>(Events.SymbolsIndex, (s) => this.onStatus(s))
      .then((off) => this.unsubs.push(off))
      .catch(() => {});
  }

  onStatus(s: IndexStatus): void {
    if (!s || !s.root) return;
    this.byRoot.update((m) => {
      const next = { ...m };
      // idle/ready roots leave the map: they have nothing to show
      if (s.state === "indexing" || s.state === "error") next[s.root] = s;
      else delete next[s.root];
      return next;
    });
  }
}
