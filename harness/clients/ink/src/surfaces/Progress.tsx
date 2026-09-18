/**
 * `progress` — one surface that ticks, never one line per tick.
 *
 * `done`/`total` absent means indeterminate, and that is a different drawing:
 * a bar that invents a length would be lying about how much is left.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** A bar `cells` wide showing `done` of `total`. */
export function bar(done: number, total: number, cells: number): string {
  if (cells <= 0) return "";
  const filled = total <= 0 ? 0 : Math.round((Math.min(done, total) / total) * cells);
  return "█".repeat(filled) + "░".repeat(Math.max(0, cells - filled));
}

/** Draw a progress surface. */
export function ProgressSurface({ node, width }: SurfaceProps<"progress">): ReactElement {
  const { label, done, total } = node.kind;
  const known = typeof done === "number" && typeof total === "number" && total > 0;
  const count = known ? `${done}/${total}` : "…";

  if (!known) {
    return (
      <Box width={width}>
        <Text>{clip(`${label} …`, width)}</Text>
      </Box>
    );
  }

  // Narrow: the label says what is happening and the count says how far. A bar
  // that squeezed those out would be decoration standing in for information.
  if (label.length + count.length + 12 > width) {
    return (
      <Box width={width}>
        <Text>
          <Text dimColor>{clip(label, Math.max(1, width - count.length - 1))}</Text>
          <Text> {count}</Text>
        </Text>
      </Box>
    );
  }

  const cells = Math.min(20, width - label.length - count.length - 2);
  return (
    <Box width={width}>
      <Text>
        <Text color="cyan">{bar(done ?? 0, total ?? 0, cells)}</Text>
        <Text> {count} </Text>
        <Text dimColor>{clip(label, Math.max(0, width - cells - count.length - 2))}</Text>
      </Text>
    </Box>
  );
}
