/**
 * `stream` — an append-only channel from a child process.
 *
 * KNOWN GAP (the fixture says so too): `SurfaceKind::Stream` names a channel
 * and carries no body, so there is nothing in the store for the bytes to land
 * in and nothing here to draw them from. What this component can honestly show
 * is the channel and its state. A body field would be a protocol change in
 * `orrery-proto`, not a change here.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";
import { clip, statusColour } from "../theme.js";

/** Draw a stream surface. */
export function StreamSurface({ node, width }: SurfaceProps<"stream">): ReactElement {
  const running = node.status === "running";
  const label = `⟨${node.kind.id}⟩`;
  return (
    <Box width={width}>
      <Text color={statusColour(node.status)}>{clip(label, width)}</Text>
      <Text dimColor>{clip(running ? " streaming…" : " closed", Math.max(0, width - label.length))}</Text>
    </Box>
  );
}
