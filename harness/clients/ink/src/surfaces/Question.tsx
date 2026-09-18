/**
 * `question` — the model asking, with the answers it will accept.
 *
 * Answering does not write state. The selection leaves as an `intent` the
 * kernel validates, which is the whole of a client's write path; AG-UI's
 * shared state is bidirectional and ours is not.
 */

import { Box, Text, useInput } from "ink";
import { useState, type ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** Draw a question surface. */
export function QuestionSurface({
  node,
  width,
  active = false,
  onIntent,
}: SurfaceProps<"question">): ReactElement {
  const { prompt, choices = [], multi, free, default: fallback, deadline_ms } = node.kind;
  const start = Math.max(0, choices.findIndex((c) => c.value === fallback));
  const [at, setAt] = useState(start);
  const [picked, setPicked] = useState<string[]>([]);
  const [typed, setTyped] = useState("");

  const send = (value: unknown): void => {
    if (node.id) onIntent?.(node.id, value);
  };

  useInput(
    (input, key) => {
      if (key.upArrow) setAt((i) => (i + choices.length - 1) % Math.max(1, choices.length));
      else if (key.downArrow) setAt((i) => (i + 1) % Math.max(1, choices.length));
      else if (input === " " && multi) {
        const value = choices[at]?.value;
        if (value) {
          setPicked((p) => (p.includes(value) ? p.filter((v) => v !== value) : [...p, value]));
        }
      } else if (key.return) {
        if (multi) send(picked);
        else if (free && typed) send(typed);
        else send(choices[at]?.value ?? fallback ?? "");
      } else if (key.backspace || key.delete) setTyped((t) => t.slice(0, -1));
      else if (free && input && !key.ctrl) setTyped((t) => t + input);
    },
    { isActive: active },
  );

  const seconds = typeof deadline_ms === "number" && deadline_ms > 0 ? Math.round(deadline_ms / 1000) : null;
  return (
    <Box width={width} flexDirection="column">
      <Text bold>{clip(prompt, width)}</Text>
      {choices.map((choice, i) => {
        const on = multi ? picked.includes(choice.value) : i === at;
        const mark = multi ? (picked.includes(choice.value) ? "[x]" : "[ ]") : i === at ? "❯" : " ";
        return (
          <Text key={choice.value} color={active && on ? "cyan" : undefined}>
            {`${mark} `}
            {clip(choice.label, Math.max(1, width - mark.length - 1))}
          </Text>
        );
      })}
      {free ? (
        <Text>
          <Text dimColor>other: </Text>
          {clip(typed, Math.max(1, width - 7))}
        </Text>
      ) : null}
      <Text dimColor>
        {clip(
          [
            multi ? "space pick · enter send" : "↑↓ enter",
            fallback ? `default ${fallback}` : null,
            seconds ? `${seconds}s` : null,
          ]
            .filter(Boolean)
            .join(" · "),
          width,
        )}
      </Text>
    </Box>
  );
}
