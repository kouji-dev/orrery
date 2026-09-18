/**
 * The consent bar.
 *
 * Deliberately not a `question` surface. A question is the model asking; this
 * is policy stopping an action, with a different vocabulary (`allow-once`,
 * `deny-always`), a deadline, and a border, so nobody answers one thinking they
 * answered the other.
 */

import { Box, Text, useInput } from "ink";
import { useState, type ReactElement } from "react";

import type { PromptView } from "@orrery/client";
import type { ConsentAnswerKind } from "@orrery/protocol";

import { clip } from "./theme.js";

/** The four answers, in the order they are offered. */
export const ANSWERS: ConsentAnswerKind[] = ["allow-once", "allow-always", "deny", "deny-always"];

/** The bar. */
export function ConsentBar({
  prompt,
  width,
  active,
  onAnswer,
}: {
  prompt: PromptView;
  width: number;
  active: boolean;
  onAnswer: (prompt: string, answer: ConsentAnswerKind) => void;
}): ReactElement {
  const [at, setAt] = useState(0);

  useInput(
    (input, key) => {
      if (key.leftArrow) setAt((i) => (i + ANSWERS.length - 1) % ANSWERS.length);
      else if (key.rightArrow) setAt((i) => (i + 1) % ANSWERS.length);
      else if (key.return) onAnswer(prompt.id, ANSWERS[at]!);
      else if (input === "a") onAnswer(prompt.id, "allow-once");
      else if (input === "d") onAnswer(prompt.id, "deny");
    },
    { isActive: active },
  );

  const seconds = Math.round(prompt.deadline_ms / 1000);
  return (
    <Box flexDirection="column" borderStyle="round" borderColor="yellow" paddingX={1}>
      <Text color="yellow" bold>
        CONSENT · {clip(prompt.reason, Math.max(8, width - 16))}
      </Text>
      <Box flexDirection="row">
        {ANSWERS.map((answer, i) => (
          <Text key={answer} inverse={active && i === at} color={answer.startsWith("deny") ? "red" : "green"}>
            {` ${answer} `}
          </Text>
        ))}
      </Box>
      <Text dimColor>{seconds > 0 ? `${seconds}s to answer · ` : ""}a allow · d deny · ← → enter</Text>
    </Box>
  );
}
