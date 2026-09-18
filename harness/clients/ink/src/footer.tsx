/**
 * The one always-visible line: what the turn cost and what the keys do.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import { clip } from "./theme.js";

/** What a finished turn reported. AG-UI carries it on `RUN_FINISHED`. */
export interface Usage {
  input_tokens?: number;
  output_tokens?: number;
  cache_hits?: number;
}

function tokens(usage: Usage | null): string {
  if (!usage) return "tokens —";
  const used = (usage.input_tokens ?? 0) + (usage.output_tokens ?? 0);
  const cached = usage.cache_hits ?? 0;
  return cached > 0 ? `tokens ${used} · cached ${cached}` : `tokens ${used}`;
}

/** The footer. */
export function Footer({
  usage,
  live,
  gaps,
  width,
}: {
  usage: Usage | null;
  live: string | null;
  gaps: number;
  width: number;
}): ReactElement {
  const parts = [tokens(usage), live ? `${live} running` : "idle"];
  if (gaps > 0) parts.push(`${gaps} gap${gaps === 1 ? "" : "s"}`);
  parts.push(live ? "^C cancel" : "^C ^C quit");
  return (
    <Box>
      <Text dimColor>{clip(parts.join(" · "), width)}</Text>
    </Box>
  );
}
