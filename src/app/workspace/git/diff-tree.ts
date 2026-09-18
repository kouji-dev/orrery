/**
 * ONE tree model for every changed-file list in the app — the working-tree
 * "Changed · N" list, the single-commit file list, and the multi-commit
 * compare. They used to build their own (the compare's had no collapse and no
 * folder aggregates), so the same list read differently depending on which
 * surface you reached it from.
 *
 * A row is either a folder or a file leaf. Folder rows carry the AGGREGATE of
 * everything below them: total ±lines and, when every descendant shares one
 * state, that state — so a fully-deleted folder reads as deleted, a fully-new
 * one as added. `flattenTree` turns the nest into the visible rows, skipping
 * the children of collapsed folders.
 */

/** The minimum a list row needs. `AgentFile`, `CommitFile` and `CommitsFile`
 *  all satisfy it, which is what lets the three surfaces share one list. */
export interface DiffEntry {
  path: string;
  state: string;
  add: number;
  del: number;
  /** R only: the pre-move path. */
  oldPath?: string;
}

export interface DiffNode {
  dir: boolean;
  name: string;
  path: string;
  file?: DiffEntry;
  children: DiffNode[];
  /** Dirs: the uniform descendant state (A/D/R/M) — undefined when mixed. */
  state?: string;
  /** Dirs: aggregate line counts over every descendant file. */
  add?: number;
  del?: number;
}

export interface DiffRow {
  dir: boolean;
  name: string;
  path: string;
  depth: number;
  file?: DiffEntry;
  state?: string;
  add?: number;
  del?: number;
}

/** Dirs first then files, alphabetical at each level; folders aggregated. */
export function buildDiffTree(files: readonly DiffEntry[]): DiffNode[] {
  const root: DiffNode = { dir: true, name: "", path: "", children: [] };
  const dirAt = new Map<string, DiffNode>([["", root]]);
  for (const f of files) {
    const parts = f.path.split("/");
    let parentPath = "";
    parts.forEach((part, i) => {
      const isFile = i === parts.length - 1;
      const path = parentPath ? `${parentPath}/${part}` : part;
      if (isFile) {
        dirAt.get(parentPath)!.children.push({ dir: false, name: part, path, file: f, children: [] });
      } else if (!dirAt.has(path)) {
        const node: DiffNode = { dir: true, name: part, path, children: [] };
        dirAt.get(parentPath)!.children.push(node);
        dirAt.set(path, node);
      }
      parentPath = path;
    });
  }
  const sortRec = (nodes: DiffNode[]) => {
    nodes.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
    for (const n of nodes) if (n.dir) sortRec(n.children);
  };
  sortRec(root.children);
  aggregateRec(root.children);
  return root.children;
}

/** Bottom-up dir aggregation: ±line sums plus the uniform state (or none). */
function aggregateRec(nodes: DiffNode[]): void {
  for (const n of nodes) {
    if (!n.dir) continue;
    aggregateRec(n.children);
    let add = 0;
    let del = 0;
    let state: string | undefined;
    let uniform = true;
    for (const c of n.children) {
      add += c.dir ? (c.add ?? 0) : (c.file?.add ?? 0);
      del += c.dir ? (c.del ?? 0) : (c.file?.del ?? 0);
      const cs = c.dir ? c.state : c.file?.state;
      if (state === undefined) state = cs;
      if (cs === undefined || cs !== state) uniform = false;
    }
    n.add = add;
    n.del = del;
    n.state = uniform ? state : undefined;
  }
}

/** The visible rows: walk the tree, skipping children of collapsed folders. */
export function flattenTree(nodes: DiffNode[], isOpen: (path: string) => boolean): DiffRow[] {
  const out: DiffRow[] = [];
  const walk = (ns: DiffNode[], depth: number) => {
    for (const n of ns) {
      out.push({
        dir: n.dir,
        name: n.name,
        path: n.path,
        depth,
        file: n.file,
        state: n.state,
        add: n.add,
        del: n.del,
      });
      if (n.dir && isOpen(n.path)) walk(n.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return out;
}
