import { describe, expect, it } from "vitest";
import {
  closeFileInLeaf,
  firstLeafOf,
  group,
  leaf,
  openFileInLeaf,
  PaneLeaf,
  PaneNode,
  setAgentView,
} from "./pane-model";

// Collect leaves depth-first (a before b) for assertions.
function leaves(node: PaneNode): PaneLeaf[] {
  return node.type === "leaf" ? [node] : [...leaves(node.a), ...leaves(node.b)];
}

describe("setAgentView", () => {
  it("switches the agent's leaf to the requested view", () => {
    const root = leaf("a1", "terminal");
    const out = setAgentView(root, "a1", "diff");
    expect(leaves(out)[0].view).toBe("diff");
  });

  it("is a no-op (same identity) when a leaf already shows the view", () => {
    // a1 tiled twice — terminal AND diff: the layout already satisfies the
    // request, the user's arrangement must not be touched.
    const root = group(leaf("a1", "terminal"), leaf("a1", "diff"));
    expect(setAgentView(root, "a1", "diff")).toBe(root);
  });

  it("is a no-op (same identity) when the agent is not in the tree", () => {
    const root = group(leaf("a1", "terminal"), leaf("a2", "terminal"));
    expect(setAgentView(root, "zz", "diff")).toBe(root);
  });

  it("switches only the FIRST matching leaf and leaves other agents alone", () => {
    const root = group(group(leaf("a2", "terminal"), leaf("a1", "terminal")), leaf("a1", "terminal"));
    const out = leaves(setAgentView(root, "a1", "diff"));
    expect(out.map((l) => l.view)).toEqual(["terminal", "diff", "terminal"]);
    expect(out[0].agentId).toBe("a2"); // untouched neighbor
  });
});

describe("file tabs in a leaf", () => {
  it("openFileInLeaf adds a tab, activates it, and switches the view", () => {
    const l = leaf("a1", "terminal");
    const out = leaves(openFileInLeaf(l, l.id, "src/app.ts"))[0];
    expect(out.files).toEqual(["src/app.ts"]);
    expect(out.activeFile).toBe("src/app.ts");
    expect(out.view).toBe("file");
  });

  it("re-opening an open file re-activates it without duplicating the tab", () => {
    const l = leaf("a1", "terminal");
    let root = openFileInLeaf(l, l.id, "a.ts");
    root = openFileInLeaf(root, l.id, "b.ts");
    root = openFileInLeaf(root, l.id, "a.ts"); // again
    const out = leaves(root)[0];
    expect(out.files).toEqual(["a.ts", "b.ts"]); // no dupe, order kept
    expect(out.activeFile).toBe("a.ts");
  });

  it("only the addressed leaf gains the tab in a split", () => {
    const a = leaf("a1", "terminal");
    const b = leaf("a2", "terminal");
    const out = leaves(openFileInLeaf(group(a, b), a.id, "x.md"));
    expect(out[0].files).toEqual(["x.md"]);
    expect(out[1].files).toBeUndefined();
  });

  it("closing the active tab activates its right neighbor (then left at the end)", () => {
    const l = leaf("a1", "terminal");
    let root = openFileInLeaf(l, l.id, "a.ts");
    root = openFileInLeaf(root, l.id, "b.ts");
    root = openFileInLeaf(root, l.id, "c.ts");
    root = openFileInLeaf(root, l.id, "b.ts"); // active = b (middle)
    root = closeFileInLeaf(root, l.id, "b.ts");
    let out = leaves(root)[0];
    expect(out.files).toEqual(["a.ts", "c.ts"]);
    expect(out.activeFile).toBe("c.ts"); // right neighbor took over
    root = closeFileInLeaf(root, l.id, "c.ts");
    out = leaves(root)[0];
    expect(out.activeFile).toBe("a.ts"); // last tab → left neighbor
  });

  it("closing an INACTIVE tab keeps the active one and the view", () => {
    const l = leaf("a1", "terminal");
    let root = openFileInLeaf(l, l.id, "a.ts");
    root = openFileInLeaf(root, l.id, "b.ts"); // active = b
    root = closeFileInLeaf(root, l.id, "a.ts");
    const out = leaves(root)[0];
    expect(out.files).toEqual(["b.ts"]);
    expect(out.activeFile).toBe("b.ts");
    expect(out.view).toBe("file");
  });

  it("closing the last tab falls back to the terminal view", () => {
    const l = leaf("a1", "terminal");
    let root = openFileInLeaf(l, l.id, "a.ts");
    root = closeFileInLeaf(root, l.id, "a.ts");
    const out = leaves(root)[0];
    expect(out.files).toEqual([]);
    expect(out.activeFile).toBeNull();
    expect(out.view).toBe("terminal");
  });

  it("closing a file while the user is on terminal/diff leaves the view alone", () => {
    const l = leaf("a1", "terminal");
    const opened = openFileInLeaf(l, l.id, "a.ts") as PaneLeaf;
    // user flipped back to the terminal; the tab stays open in the strip
    const root = closeFileInLeaf({ ...opened, view: "terminal" }, l.id, "a.ts");
    expect(leaves(root)[0].view).toBe("terminal");
  });

  it("firstLeafOf finds the depth-first leaf for an agent", () => {
    const a = leaf("a1", "terminal");
    const b = leaf("a2", "terminal");
    const c = leaf("a1", "diff");
    const root = group(group(b, a), c);
    expect(firstLeafOf(root, "a1")).toBe(a.id);
    expect(firstLeafOf(root, "zz")).toBeNull();
  });
});
