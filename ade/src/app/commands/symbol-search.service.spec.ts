/**
 * SymbolSearchService — the `symbols_search` adapter (M2). No TestBed: the
 * service is built in a bare injector with a fake bridge whose replies the
 * test releases by hand, so the generation guard is observable.
 */
import { DestroyRef, Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { SymbolHit } from "../models";
import { SYMBOL_LIMIT, SymbolSearchService, toSymbolRow } from "./symbol-search.service";

type Reply = { resolve: (hits: SymbolHit[]) => void; reject: (e: unknown) => void; args: Record<string, unknown> };

class FakeBridge implements Bridge {
  readonly pending: Reply[] = [];
  invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R> {
    if (command !== "symbols_search") return Promise.reject(new Error("unexpected " + command));
    return new Promise<R>((resolve, reject) => {
      this.pending.push({ resolve: resolve as (h: SymbolHit[]) => void, reject, args: payload ?? {} });
    });
  }
  on(): Promise<() => void> {
    return Promise.resolve(() => {});
  }
  pickDirectory(): Promise<string | null> {
    return Promise.resolve(null);
  }
  pickFile(): Promise<string | null> {
    return Promise.resolve(null);
  }
}

const hit = (over: Partial<SymbolHit> & { name: string }): SymbolHit => ({
  kind: "fn",
  path: "src/a.ts",
  line: 0,
  col: 0,
  container: null,
  agentId: null,
  root: null,
  ...over,
});

function make() {
  const bridge = new FakeBridge();
  const injector = Injector.create({
    providers: [
      { provide: BRIDGE, useValue: bridge },
      { provide: DestroyRef, useValue: { onDestroy: vi.fn() } },
    ],
  });
  const svc = runInInjectionContext(injector, () => new SymbolSearchService());
  return { svc, bridge };
}

const tick = () => new Promise((r) => setTimeout(r, 0));
const WT = { kind: "worktree" as const, agentId: "ag-1", projectId: "p-1" };

describe("toSymbolRow", () => {
  it("maps a 0-based hit to a 1-based, keyed row", () => {
    const row = toSymbolRow(hit({ name: "handleFetch", kind: "method", path: "src/x.ts", line: 41, col: 6, container: "Store", agentId: "ag-1", root: "wt-a" }));
    expect(row).toEqual({
      key: "ag-1|src/x.ts:41:handleFetch",
      name: "handleFetch",
      kind: "method",
      path: "src/x.ts",
      line: 42,
      col: 7,
      container: "Store",
      agentId: "ag-1",
      root: "wt-a",
    });
  });

  it("normalises missing optionals to null", () => {
    const row = toSymbolRow({ name: "x", kind: "fn", path: "a", line: 0, col: 0 } as SymbolHit);
    expect(row.container).toBeNull();
    expect(row.agentId).toBeNull();
    expect(row.root).toBeNull();
    expect(row.key).toBe("|a:0:x");
  });
});

describe("SymbolSearchService", () => {
  it("sends one symbols_search with the scope, trimmed query and the cap", async () => {
    const { svc, bridge } = make();
    svc.search("  fetch ", WT);
    await tick();
    expect(bridge.pending).toHaveLength(1);
    expect(bridge.pending[0].args).toEqual({ scope: WT, query: "fetch", limit: SYMBOL_LIMIT });
    expect(svc.busy()).toBe(true);
    bridge.pending[0].resolve([hit({ name: "fetchAll", line: 3 })]);
    await tick();
    expect(svc.busy()).toBe(false);
    expect(svc.hits().map((h) => [h.name, h.line])).toEqual([["fetchAll", 4]]);
    expect(svc.truncated()).toBe(false);
  });

  it("drops a stale reply: the newer query's rows win, whatever order the replies land in", async () => {
    const { svc, bridge } = make();
    svc.search("a", WT);
    svc.search("ab", WT);
    await tick();
    expect(bridge.pending).toHaveLength(2);
    bridge.pending[1].resolve([hit({ name: "abc" })]);
    await tick();
    expect(svc.hits().map((h) => h.name)).toEqual(["abc"]);
    expect(svc.busy()).toBe(false);
    // the first query answers late — ignored, busy stays settled
    bridge.pending[0].resolve([hit({ name: "aaa" })]);
    await tick();
    expect(svc.hits().map((h) => h.name)).toEqual(["abc"]);
    expect(svc.busy()).toBe(false);
  });

  it("cancel() abandons the in-flight query and keeps what is listed", async () => {
    const { svc, bridge } = make();
    svc.search("x", WT);
    await tick();
    bridge.pending[0].resolve([hit({ name: "x1" })]);
    await tick();
    svc.search("xy", WT);
    await tick();
    svc.cancel();
    expect(svc.busy()).toBe(false);
    bridge.pending[1].resolve([hit({ name: "xy1" })]);
    await tick();
    expect(svc.hits().map((h) => h.name)).toEqual(["x1"]);
  });

  it("an empty query clears without a request; a missing root is an error, not a request", async () => {
    const { svc, bridge } = make();
    svc.search("x", WT);
    await tick();
    bridge.pending[0].resolve([hit({ name: "x1" })]);
    await tick();
    svc.search("   ", WT);
    await tick();
    expect(svc.hits()).toEqual([]);
    expect(bridge.pending).toHaveLength(1);

    svc.search("x", { kind: "worktree", agentId: null, projectId: "p" });
    await tick();
    expect(svc.error()).toBe("open a worktree to search its symbols");
    svc.search("x", { kind: "project", agentId: null, projectId: null });
    await tick();
    expect(svc.error()).toBe("no project to search");
    expect(bridge.pending).toHaveLength(1);
  });

  it("surfaces a failed invoke as the error and marks a full page as capped", async () => {
    const { svc, bridge } = make();
    svc.search("x", WT);
    await tick();
    bridge.pending[0].reject(new Error("index not ready"));
    await tick();
    expect(svc.error()).toBe("index not ready");
    expect(svc.busy()).toBe(false);

    svc.search("y", WT);
    await tick();
    bridge.pending[1].resolve(Array.from({ length: SYMBOL_LIMIT }, (_, i) => hit({ name: "y" + i, line: i })));
    await tick();
    expect(svc.error()).toBeNull();
    expect(svc.truncated()).toBe(true);
    expect(svc.hits()).toHaveLength(SYMBOL_LIMIT);
  });
});
