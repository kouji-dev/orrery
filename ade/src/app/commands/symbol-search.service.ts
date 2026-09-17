import { DestroyRef, inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { SymbolHit } from "../models";
import { ScopeKind } from "../shared/scope";

export type { SymbolHit };

/** One Search-Everywhere symbol row: a backend hit with 1-BASED line/col
 *  (what the tab prints and `openFileAt` takes) and a stable render key. */
export interface SymbolRow {
  /** agent + path + line + name — stable across re-queries. */
  key: string;
  name: string;
  kind: string;
  path: string;
  line: number;
  col: number;
  container: string | null;
  /** Worktree the hit came from (null = the project checkout). */
  agentId: string | null;
  /** Human label of that root (agent name); null = project checkout. */
  root: string | null;
}

/** Backend cap per query. Past it the row list says "capped". */
export const SYMBOL_LIMIT = 200;

/** A backend hit (0-based) → a row (1-based, keyed). */
export function toSymbolRow(h: SymbolHit): SymbolRow {
  return {
    key: (h.agentId ?? "") + "|" + h.path + ":" + h.line + ":" + h.name,
    name: h.name,
    kind: h.kind,
    path: h.path,
    line: h.line + 1,
    col: h.col + 1,
    container: h.container ?? null,
    agentId: h.agentId ?? null,
    root: h.root ?? null,
  };
}

/**
 * Symbol lookup over the tree-sitter index (`symbols_search`, M2): one
 * invoke per query, answered from the backend's in-memory per-root maps.
 *
 * Root-provided because the Search-Everywhere overlay is destroyed on every
 * close; the generation guard below is what keeps a reply that lands after a
 * newer query (or after Stop) from overwriting the list.
 */
@Injectable({ providedIn: "root" })
export class SymbolSearchService {
  private bridge = inject(BRIDGE);

  readonly hits = signal<SymbolRow[]>([]);
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly truncated = signal(false);

  /** Bumped per run AND per cancel — a reply whose gen is stale is dropped. */
  private gen = 0;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.cancel());
  }

  /** Run a symbol search. Fire-and-forget: results land in `hits()`. */
  search(q: string, opts: { kind: ScopeKind; agentId: string | null; projectId: string | null }): void {
    void this.run(q, opts);
  }

  /** Abandon the in-flight query, keeping whatever is already listed. */
  cancel(): void {
    this.gen++;
    this.busy.set(false);
  }

  private async run(
    q: string,
    opts: { kind: ScopeKind; agentId: string | null; projectId: string | null },
  ): Promise<void> {
    const gen = ++this.gen;
    this.error.set(null);
    this.truncated.set(false);
    const query = q.trim();
    if (!query) {
      this.hits.set([]);
      this.busy.set(false);
      return;
    }
    if (opts.kind === "worktree" && !opts.agentId) {
      this.error.set("open a worktree to search its symbols");
      return;
    }
    if (opts.kind !== "worktree" && !opts.projectId) {
      this.error.set("no project to search");
      return;
    }
    this.busy.set(true);
    try {
      const res = await this.bridge.invoke<SymbolHit[]>(Commands.SymbolsSearch, {
        scope: { kind: opts.kind, agentId: opts.agentId, projectId: opts.projectId },
        query,
        limit: SYMBOL_LIMIT,
      });
      if (gen !== this.gen) return;
      const rows = (res ?? []).map(toSymbolRow);
      this.hits.set(rows);
      this.truncated.set(rows.length >= SYMBOL_LIMIT);
    } catch (e) {
      if (gen !== this.gen) return;
      this.error.set((e as { message?: string })?.message ?? "symbol search failed");
    } finally {
      if (gen === this.gen) this.busy.set(false);
    }
  }
}
