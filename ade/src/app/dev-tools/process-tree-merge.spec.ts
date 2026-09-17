import { describe, expect, it } from "vitest";
import { ProcessNode, ProcessTreeSnapshot } from "../models";
import { mergeRoots } from "./process-tree-merge";

function node(p: Partial<ProcessNode> & { pid: number; name: string }): ProcessNode {
  return {
    note: null,
    cpu: 0,
    privBytes: 0,
    rssBytes: 0,
    subtreeCpu: p.cpu ?? 0,
    subtreePrivBytes: p.privBytes ?? 0,
    subtreeProcs: 1,
    detached: false,
    excluded: false,
    children: [],
    ...p,
  };
}

const MB = 2 ** 20;

// app root: orrery.exe (2% / 100MB) with a webview child (1.5% / 400MB)
// agents: two subtrees — the backend carves them out as separate roots
function snapshot(): ProcessTreeSnapshot {
  const webview = node({ pid: 11, name: "msedgewebview2.exe", cpu: 1.5, privBytes: 400 * MB });
  const app = node({
    pid: 10,
    name: "orrery.exe",
    cpu: 2,
    privBytes: 100 * MB,
    subtreeCpu: 3.5,
    subtreePrivBytes: 500 * MB,
    subtreeProcs: 2,
    children: [webview],
  });
  const ag1 = node({ pid: 20, name: "node.exe", cpu: 4, privBytes: 600 * MB, subtreeCpu: 4.5, subtreePrivBytes: 650 * MB, subtreeProcs: 3 });
  const ag2 = node({ pid: 30, name: "claude.exe", cpu: 0.5, privBytes: 50 * MB, subtreeProcs: 1 });
  return {
    roots: [
      { id: "app", label: "Orrery", node: app },
      { id: "ag-1", label: "ticket-42", node: ag1 },
      { id: "ag-2", label: "pty fix", node: ag2 },
    ],
    tsMs: 1,
  };
}

describe("mergeRoots", () => {
  it("returns null with no roots (nothing sampled yet)", () => {
    expect(mergeRoots({ roots: [], tsMs: 0 })).toBeNull();
  });

  it("builds one Orrery App root whose numbers are the recursive total of every subtree", () => {
    const root = mergeRoots(snapshot())!;
    expect(root.name).toBe("Orrery App");
    // 3.5 (app subtree) + 4.5 + 0.5 — both own and subtree columns carry the total
    expect(root.cpu).toBeCloseTo(8.5);
    expect(root.subtreeCpu).toBeCloseTo(8.5);
    expect(root.privBytes).toBe((500 + 650 + 50) * MB);
    expect(root.subtreePrivBytes).toBe((500 + 650 + 50) * MB);
    expect(root.subtreeProcs).toBe(2 + 3 + 1);
  });

  it("nests the app subtree first, then every agent subtree, parentage intact", () => {
    const root = mergeRoots(snapshot())!;
    expect(root.children.map((c) => c.name)).toEqual(["orrery.exe", "node.exe", "claude.exe"]);
    expect(root.children[0].children[0].name).toBe("msedgewebview2.exe");
  });

  it("still merges when the app root is absent (agents only)", () => {
    const t = snapshot();
    t.roots = t.roots.filter((r) => r.id !== "app");
    const root = mergeRoots(t)!;
    expect(root.children.map((c) => c.name)).toEqual(["node.exe", "claude.exe"]);
    expect(root.subtreePrivBytes).toBe((650 + 50) * MB);
  });
});
