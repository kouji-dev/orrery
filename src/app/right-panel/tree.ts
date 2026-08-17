// Build a nested file tree from a flat path list + a {path: state} map.

export interface TreeItem {
  name: string;
  dir: boolean;
  path: string;
  state?: "A" | "M" | "D";
  children: TreeItem[];
}

interface RawNode {
  __dir: boolean;
  __children: Record<string, RawNode>;
  __path: string;
  __state?: "A" | "M" | "D";
}

export function buildTree(paths: string[], stateMap: Record<string, "A" | "M" | "D">): TreeItem[] {
  const root: Record<string, RawNode> = {};
  paths.forEach((p) => {
    const isDir = p.endsWith("/");
    const parts = p.replace(/\/$/, "").split("/");
    let node = root;
    parts.forEach((part, i) => {
      const last = i === parts.length - 1;
      if (!node[part]) {
        node[part] = {
          __dir: !last || isDir,
          __children: {},
          __path: parts.slice(0, i + 1).join("/") + (!last || isDir ? "/" : ""),
        };
      }
      if (last) node[part].__state = stateMap[p];
      node = node[part].__children;
    });
  });

  const toArr = (obj: Record<string, RawNode>): TreeItem[] =>
    Object.keys(obj)
      .map((k) => {
        const v = obj[k];
        return {
          name: k,
          dir: v.__dir,
          path: v.__path,
          state: v.__state,
          children: v.__dir ? toArr(v.__children) : [],
        };
      })
      .sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));

  return toArr(root);
}

export function countChanged(node: TreeItem): number {
  let n = 0;
  const walk = (x: TreeItem) => {
    if (!x.dir && x.state) n++;
    if (x.state && x.dir) n++;
    x.children.forEach(walk);
  };
  node.children.forEach(walk);
  return n;
}

export const STATE_COLOR: Record<string, string> = {
  A: "var(--vcs-added)",
  M: "var(--vcs-modified)",
  D: "var(--vcs-deleted)",
};
