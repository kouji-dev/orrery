/**
 * `diff` — a change to one file.
 *
 * The wire carries line kinds without markers, deliberately: the marker is the
 * renderer's, which is why the same description can be drawn unified here and
 * side by side somewhere else.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { DiffLineKind } from "@orrery/protocol";

import type { SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** The marker and colour for a line kind. */
export function marker(kind: DiffLineKind): { sign: string; colour: string | undefined } {
  switch (kind) {
    case "add":
      return { sign: "+", colour: "green" };
    case "remove":
      return { sign: "-", colour: "red" };
    default:
      return { sign: " ", colour: undefined };
  }
}

/** Draw a diff surface. */
export function DiffSurface({ node, width }: SurfaceProps<"diff">): ReactElement {
  const { path, hunks = [] } = node.kind;
  const added = hunks.flatMap((h) => h.lines ?? []).filter((l) => l.kind === "add").length;
  const removed = hunks.flatMap((h) => h.lines ?? []).filter((l) => l.kind === "remove").length;
  return (
    <Box width={width} flexDirection="column">
      <Text bold>
        {clip(path, Math.max(1, width - 10))} <Text color="green">+{added}</Text>{" "}
        <Text color="red">-{removed}</Text>
      </Text>
      {hunks.map((hunk, i) => (
        <Box key={i} flexDirection="column">
          <Text dimColor>
            {clip(
              `@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@`,
              width,
            )}
          </Text>
          {(hunk.lines ?? []).map((line, j) => {
            const { sign, colour } = marker(line.kind);
            return (
              <Text key={j} color={colour}>
                {clip(`${sign}${line.text}`, width)}
              </Text>
            );
          })}
        </Box>
      ))}
    </Box>
  );
}
