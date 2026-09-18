/**
 * The input line.
 *
 * `^C` means *stop the turn*, not *kill the client*: a person who interrupts a
 * tool call has not asked to lose their transcript. Only a second `^C` with
 * nothing running exits, and `exitOnCtrlC` is off so Ink does not do it first.
 */

import { Box, Text, useApp, useInput } from "ink";
import { useState, type ReactElement } from "react";

/** The composer. */
export function Composer({
  active,
  busy,
  onSubmit,
  onCancel,
}: {
  active: boolean;
  /** Whether a turn is in flight: that is what `^C` cancels. */
  busy: boolean;
  onSubmit: (text: string) => void;
  onCancel: () => void;
}): ReactElement {
  const [value, setValue] = useState("");
  const [armed, setArmed] = useState(false);
  const { exit } = useApp();

  useInput(
    (input, key) => {
      if (key.ctrl && input === "c") {
        if (busy) {
          onCancel();
          setArmed(false);
          return;
        }
        if (armed) {
          exit();
          return;
        }
        setArmed(true);
        return;
      }
      setArmed(false);
      if (key.return) {
        const text = value.trim();
        setValue("");
        if (text) onSubmit(text);
        return;
      }
      if (key.backspace || key.delete) {
        setValue((v) => v.slice(0, -1));
        return;
      }
      if (key.escape || key.tab || key.upArrow || key.downArrow) return;
      if (input) setValue((v) => v + input);
    },
    { isActive: active },
  );

  return (
    <Box>
      <Text color={active ? "cyan" : "gray"}>{"› "}</Text>
      <Text>{value}</Text>
      {armed ? <Text dimColor> (^C again to quit)</Text> : null}
    </Box>
  );
}
