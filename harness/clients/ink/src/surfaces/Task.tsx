/**
 * `task` — the checklist the agent keeps. One line per item, in the order the
 * kernel sent them; a tick is one op, not a new list.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";
import { clip, statusColour, statusGlyph } from "../theme.js";

/** Draw a task surface. */
export function TaskSurface({ node, width }: SurfaceProps<"task">): ReactElement {
  const items = node.kind.items ?? [];
  const done = items.filter((i) => i.status === "done").length;
  return (
    <Box width={width} flexDirection="column">
      <Text dimColor>{clip(`${done}/${items.length} done`, width)}</Text>
      {items.map((item) => (
        <Text key={item.id}>
          <Text color={statusColour(item.status)}>{statusGlyph(item.status)} </Text>
          <Text
            dimColor={item.status === "pending"}
            strikethrough={item.status === "cancelled"}
          >
            {clip(item.label, Math.max(1, width - 2))}
          </Text>
        </Text>
      ))}
    </Box>
  );
}
