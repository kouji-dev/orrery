/**
 * `stack` — the container. A tool call is one of these: the arguments are the
 * first child and the result is the second, which is why no component here
 * knows what a tool call is.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import { SurfaceNode } from "./index.js";
import { drawable, type SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** Draw a stack surface. */
export function StackSurface({ node, width, active, onIntent }: SurfaceProps<"stack">): ReactElement {
  const { title, children = [], collapsed, dir } = node.kind;
  const head = title ? <Text bold>{clip(title, width)}</Text> : null;

  if (collapsed) {
    return (
      <Box width={width} flexDirection="column">
        {head}
        <Text dimColor>{clip(`▸ ${children.length} hidden`, width)}</Text>
      </Box>
    );
  }

  // A row of children only fits when there is room for each; below that a
  // column is the honest drawing.
  const room = Math.floor((width - (children.length - 1)) / Math.max(1, children.length));
  const row = dir === "row" && room >= 12;
  return (
    <Box width={width} flexDirection="column">
      {head}
      <Box flexDirection={row ? "row" : "column"}>
        {children.map((child, i) => (
          <Box key={child.id ?? i} flexDirection="column" marginRight={row && i < children.length - 1 ? 1 : 0}>
            <SurfaceNode
              node={drawable(child)}
              width={row ? room : width}
              active={active}
              onIntent={onIntent}
            />
          </Box>
        ))}
      </Box>
    </Box>
  );
}
