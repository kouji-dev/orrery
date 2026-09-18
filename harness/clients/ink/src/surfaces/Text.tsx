/**
 * `text` — a run of text with a hint about how it is meant.
 *
 * The hint is never a colour on the wire (§6.3); it becomes one here, and
 * `theme.ts` is the only place that decides which.
 */

import { Text } from "ink";
import type { ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";
import { styleProps } from "../theme.js";

/** Draw a text surface. */
export function TextSurface({ node, width }: SurfaceProps<"text">): ReactElement {
  const { value, style } = node.kind;
  return (
    <Text {...styleProps(style)} wrap="wrap">
      {trimTrailing(value, width)}
    </Text>
  );
}

/** Drop the trailing newline a tool's output usually carries. */
function trimTrailing(value: string, width: number): string {
  const text = value.replace(/\n+$/, "");
  return width > 0 ? text : "";
}
