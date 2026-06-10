// Split / tiling pane tree for the workspace. A tab's workspace is a binary tree
// of leaves (one agent + view) and splits (a/b children with a drag ratio).

import { Agent, Project } from "../models";

export type PaneView = "terminal" | "diff";

export interface PaneLeaf {
  type: "leaf";
  id: string;
  agentId: string | null;
  view: PaneView;
}
export interface PaneSplit {
  type: "split";
  id: string;
  dir: "v" | "h"; // v = side-by-side columns, h = stacked rows
  ratio: number; // 0..1 size of child `a`
  a: PaneNode;
  b: PaneNode;
}
export type PaneNode = PaneLeaf | PaneSplit;

let _pid = 0;
const nid = (): string => "pane" + ++_pid;

/** A fresh leaf pane for `agentId` (null = unassigned) showing `view`. */
export function leaf(agentId: string | null, view: PaneView = "terminal"): PaneLeaf {
  return { type: "leaf", id: nid(), agentId, view };
}

/** A fresh split combining two whole trees (used when grouping/merging tabs). */
export function group(a: PaneNode, b: PaneNode, dir: "v" | "h" = "v", ratio = 0.5): PaneSplit {
  return { type: "split", id: nid(), dir, ratio, a, b };
}

/** Split the leaf with `id` in two (dir), inserting `nl` as the new second child. */
export function splitLeaf(node: PaneNode, id: string, dir: "v" | "h", nl: PaneLeaf): PaneNode {
  if (node.type === "leaf") {
    return node.id === id ? { type: "split", id: nid(), dir, ratio: 0.5, a: node, b: nl } : node;
  }
  return { ...node, a: splitLeaf(node.a, id, dir, nl), b: splitLeaf(node.b, id, dir, nl) };
}

/** Remove the leaf `id`; its sibling takes the split's place. */
export function removeLeaf(node: PaneNode, id: string): PaneNode {
  if (node.type === "leaf") return node;
  const { a, b } = node;
  if (a.type === "leaf" && a.id === id) return b;
  if (b.type === "leaf" && b.id === id) return a;
  return { ...node, a: removeLeaf(a, id), b: removeLeaf(b, id) };
}

/** Patch a leaf (e.g. change its agentId or view). */
export function patchLeaf(node: PaneNode, id: string, patch: Partial<PaneLeaf>): PaneNode {
  if (node.type === "leaf") return node.id === id ? { ...node, ...patch } : node;
  return { ...node, a: patchLeaf(node.a, id, patch), b: patchLeaf(node.b, id, patch) };
}

/** Set a split node's divider ratio. */
export function setRatio(node: PaneNode, id: string, ratio: number): PaneNode {
  if (node.type === "leaf") return node;
  if (node.id === id) return { ...node, ratio };
  return { ...node, a: setRatio(node.a, id, ratio), b: setRatio(node.b, id, ratio) };
}

export function countLeaves(node: PaneNode): number {
  return node.type === "leaf" ? 1 : countLeaves(node.a) + countLeaves(node.b);
}

export type DropSide = "left" | "right" | "top" | "bottom" | "center";

/**
 * Split a specific leaf toward a side, inserting `nl`. `center` replaces the
 * leaf outright. left/right → vertical split (columns); top/bottom → horizontal
 * (rows); left/top put the new pane first.
 */
export function splitLeafSide(node: PaneNode, id: string, side: DropSide, nl: PaneLeaf): PaneNode {
  if (node.type === "leaf") {
    if (node.id !== id) return node;
    if (side === "center") return nl;
    const dir = side === "left" || side === "right" ? "v" : "h";
    const first = side === "left" || side === "top";
    return { type: "split", id: nid(), dir, ratio: 0.5, a: first ? nl : node, b: first ? node : nl };
  }
  return { ...node, a: splitLeafSide(node.a, id, side, nl), b: splitLeafSide(node.b, id, side, nl) };
}

/** The agentId shown in the leaf with `paneId` (null if unassigned / not found). */
export function leafAgentOf(node: PaneNode, paneId: string): string | null {
  let r: string | null = null;
  const walk = (n: PaneNode) => {
    if (n.type === "leaf") {
      if (n.id === paneId) r = n.agentId;
    } else {
      walk(n.a);
      walk(n.b);
    }
  };
  walk(node);
  return r;
}

/** Show `view` for `agentId`: no-op when one of its leaves already does (the
 *  user's layout wins); otherwise its first leaf switches view in place. */
export function setAgentView(node: PaneNode, agentId: string, view: PaneView): PaneNode {
  let satisfied = false;
  let targetId: string | null = null;
  const walk = (n: PaneNode) => {
    if (n.type === "leaf") {
      if (n.agentId === agentId) {
        if (n.view === view) satisfied = true;
        else targetId ??= n.id;
      }
    } else {
      walk(n.a);
      walk(n.b);
    }
  };
  walk(node);
  if (satisfied || targetId === null) return node;
  return patchLeaf(node, targetId, { view });
}

/** Remove every leaf showing `agentId`; each one's sibling takes the split's place. */
export function dropAgent(node: PaneNode, agentId: string): PaneNode {
  if (node.type === "leaf") return node;
  const { a, b } = node;
  if (a.type === "leaf" && a.agentId === agentId) return dropAgent(b, agentId);
  if (b.type === "leaf" && b.agentId === agentId) return dropAgent(a, agentId);
  return { ...node, a: dropAgent(a, agentId), b: dropAgent(b, agentId) };
}

/** All agent ids currently shown anywhere in the tree. */
export function treeAgentIds(node: PaneNode): string[] {
  const out: string[] = [];
  const walk = (n: PaneNode) => {
    if (n.type === "leaf") {
      if (n.agentId) out.push(n.agentId);
    } else {
      walk(n.a);
      walk(n.b);
    }
  };
  walk(node);
  return out;
}

/**
 * The manager surface the recursive pane node calls into — defined as an
 * interface so the node doesn't depend on the manager component class (avoids a
 * circular import). PaneManagerComponent implements it.
 */
export interface PaneCtx {
  agents(): Agent[];
  projects(): Project[];
  focusId(): string | null;
  canClose(): boolean;
  onSplit(leafId: string, dir: "v" | "h"): void;
  onClose(leafId: string): void;
  onAgent(leafId: string, agentId: string): void;
  onView(leafId: string, view: PaneView): void;
  onRatio(splitId: string, ratio: number): void;
  onFocus(leafId: string): void;
  // drag-and-drop: a sidebar agent / tab dragged over a pane. dropTarget() is the
  // pane currently hovered + which side the new pane would land on.
  dropTarget(): { paneId: string; side: DropSide } | null;
  onDropOver(paneId: string | null, side: DropSide): void;
  onPaneDrop(paneId: string): void;
}
