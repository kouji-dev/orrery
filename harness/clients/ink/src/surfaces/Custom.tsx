/**
 * `custom` — an extension's own surface.
 *
 * With a registered renderer, that renderer draws it. Without one, the
 * fallback does — and the fallback is mandatory on the wire precisely so this
 * branch is never a blank hole in a transcript.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import { rendererFor } from "../registry.js";
import { SurfaceNode } from "./index.js";
import { drawable, type SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** Draw a custom surface. */
export function CustomSurface({
  node,
  width,
  active,
  onIntent,
}: SurfaceProps<"custom">): ReactElement {
  const { kind, payload, fallback } = node.kind;
  const Renderer = rendererFor(kind);
  if (Renderer) {
    return (
      <Box width={width} flexDirection="column">
        <Renderer kind={kind} payload={payload} width={width} onIntent={onIntent} id={node.id ?? null} />
      </Box>
    );
  }
  return (
    <Box width={width} flexDirection="column">
      <Text dimColor>{clip(`${kind} (no renderer)`, width)}</Text>
      <SurfaceNode node={drawable(fallback)} width={width} active={active} onIntent={onIntent} />
    </Box>
  );
}
