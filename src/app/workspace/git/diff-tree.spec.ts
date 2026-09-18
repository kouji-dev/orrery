import { describe, expect, it } from "vitest";
import { buildDiffTree, DiffEntry, flattenTree } from "./diff-tree";

const f = (path: string, state = "M", add = 1, del = 1): DiffEntry => ({ path, state, add, del });

describe("buildDiffTree", () => {
  it("nests by path segment, dirs before files, alphabetical at each level", () => {
    const tree = buildDiffTree([f("z.ts"), f("src/b.ts"), f("src/a.ts"), f("docs/x.md")]);
    expect(tree.map((n) => `${n.dir ? "d" : "f"}:${n.name}`)).toEqual(["d:docs", "d:src", "f:z.ts"]);
    expect(tree[1].children.map((n) => n.name)).toEqual(["a.ts", "b.ts"]);
  });

  it("aggregates ±lines up every folder level", () => {
    const tree = buildDiffTree([f("src/a/one.ts", "M", 10, 2), f("src/b.ts", "M", 5, 3)]);
    const src = tree[0];
    expect(src.add).toBe(15);
    expect(src.del).toBe(5);
    expect(src.children[0].add).toBe(10); // src/a
  });

  it("gives a folder its descendants' state only when they all share one", () => {
    const allNew = buildDiffTree([f("new/a.ts", "A"), f("new/b.ts", "A")]);
    expect(allNew[0].state).toBe("A");

    const mixed = buildDiffTree([f("mix/a.ts", "A"), f("mix/b.ts", "D")]);
    expect(mixed[0].state).toBeUndefined();
  });
});

describe("flattenTree", () => {
  const tree = buildDiffTree([f("src/deep/a.ts"), f("src/b.ts"), f("top.ts")]);

  it("walks every level when all folders are open, carrying the depth", () => {
    const rows = flattenTree(tree, () => true);
    expect(rows.map((r) => `${r.depth}:${r.name}`)).toEqual([
      "0:src",
      "1:deep",
      "2:a.ts",
      "1:b.ts",
      "0:top.ts",
    ]);
  });

  it("skips the children of a collapsed folder, and only those", () => {
    const rows = flattenTree(tree, (p) => p !== "src/deep");
    expect(rows.map((r) => r.name)).toEqual(["src", "deep", "b.ts", "top.ts"]);
  });

  it("collapsing a parent hides the whole subtree", () => {
    const rows = flattenTree(tree, (p) => p !== "src");
    expect(rows.map((r) => r.name)).toEqual(["src", "top.ts"]);
  });
});
