import { describe, expect, it } from "vitest";
import { group, leaf, PaneLeaf, PaneNode, setAgentView } from "./pane-model";

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
