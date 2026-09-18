/**
 * One turn, drawn.
 *
 * A settled turn goes through `<Static>` and is printed once; the live one is
 * redrawn. Same component either way, so scrollback and the live region cannot
 * drift apart.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { TurnView } from "@orrery/client";

import { askingSurface } from "./focus.js";
import { SurfaceNode } from "./surfaces/index.js";
import { statusColour, statusGlyph } from "./theme.js";

export function Turn({
  turn,
  width,
  active = false,
  onIntent,
}: {
  turn: TurnView;
  width: number;
  active?: boolean;
  onIntent?: (surface: string, value: unknown) => void;
}): ReactElement {
  const asking = active ? askingSurface(turn) : null;
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text dimColor>
        ── {turn.id} {turn.cancelled ? "(cancelled)" : turn.settled ? "" : "…"}
      </Text>
      {turn.surfaces.map((surface) => (
        <Box key={surface.id} flexDirection="row">
          <Text color={statusColour(surface.status)}>{statusGlyph(surface.status)} </Text>
          <Box flexDirection="column">
            <SurfaceNode
              node={surface}
              width={Math.max(4, width - 2)}
              active={asking?.id === surface.id}
              onIntent={onIntent}
            />
          </Box>
        </Box>
      ))}
      {turn.error ? (
        <Text color="red">
          error: {turn.error.message}
          {turn.error.code ? ` (${turn.error.code})` : ""}
        </Text>
      ) : null}
    </Box>
  );
}

