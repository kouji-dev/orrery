/**
 * `tree` — a hierarchy. Expansion is described by the tree and drawn by the
 * client: a closed node's children are not drawn, and opening one is one op on
 * that node's `expanded` flag rather than a new tree.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { TreeNode } from "@orrery/protocol";

import type { SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** One row of a drawn tree. */
export interface Row {
  prefix: string;
  label: string;
  open: boolean;
  leaf: boolean;
}

/** Flatten the visible part of a tree: closed nodes hide their children. */
export function rows(nodes: TreeNode[], indent = ""): Row[] {
  const out: Row[] = [];
  nodes.forEach((node, i) => {
    const last = i === nodes.length - 1;
    const children = node.children ?? [];
    out.push({
      prefix: `${indent}${last ? "└" : "├"}`,
      label: node.label,
      open: node.expanded,
      leaf: children.length === 0,
    });
    if (node.expanded && children.length > 0) {
      out.push(...rows(children, `${indent}${last ? "  " : "│ "}`));
    }
  });
  return out;
}

/** Draw a tree surface. */
export function TreeSurface({ node, width }: SurfaceProps<"tree">): ReactElement {
  const list = rows(node.kind.nodes ?? []);
  return (
    <Box width={width} flexDirection="column">
      {list.map((row, i) => {
        const glyph = row.leaf ? "·" : row.open ? "▾" : "▸";
        const head = `${row.prefix}${glyph} `;
        return (
          <Text key={i}>
            <Text dimColor>{head}</Text>
            {clip(row.label, Math.max(1, width - head.length))}
          </Text>
        );
      })}
    </Box>
  );
}
