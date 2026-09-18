/**
 * `form` — several values collected at once.
 *
 * `secret` is a field *kind*, not a flag, and this is why: the renderer has to
 * know never to echo it. The value is masked on screen and the surface never
 * logs it.
 *
 * The fields are asked in declaration order (plan 09's degradation rule), one
 * at a time, and the whole form is submitted as one `intent` when the last
 * required field has an answer.
 */

import { Box, Text, useInput } from "ink";
import { useState, type ReactElement } from "react";

import type { Field } from "@orrery/protocol";

import type { SurfaceProps } from "./kinds.js";
import { clip } from "../theme.js";

/** What a field shows before anyone types into it. */
export function initial(fields: Field[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const field of fields) {
    if (field.kind.t === "secret") out[field.name] = "";
    else out[field.name] = field.default ?? (field.kind.t === "bool" ? "false" : "");
  }
  return out;
}

/** What a field's value looks like on screen. A secret never looks like itself. */
export function shown(field: Field, value: string): string {
  if (field.kind.t === "secret") return value ? "•".repeat(Math.min(8, value.length)) : "••••";
  if (field.kind.t === "bool") return value === "true" ? "yes" : "no";
  return value;
}

/** Whether every required field has something in it. */
export function complete(fields: Field[], values: Record<string, string>): boolean {
  return fields.every((f) => !f.required || (values[f.name] ?? "").length > 0);
}

/** Draw a form surface. */
export function FormSurface({
  node,
  width,
  active = false,
  onIntent,
}: SurfaceProps<"form">): ReactElement {
  const fields = node.kind.fields ?? [];
  const [values, setValues] = useState(() => initial(fields));
  const [at, setAt] = useState(0);

  const field = fields[at];

  useInput(
    (input, key) => {
      if (!field) return;
      if (key.downArrow || key.tab) setAt((i) => Math.min(fields.length - 1, i + 1));
      else if (key.upArrow) setAt((i) => Math.max(0, i - 1));
      else if (key.return) {
        if (at < fields.length - 1) setAt(at + 1);
        else if (complete(fields, values) && node.id) onIntent?.(node.id, values);
      } else if (key.backspace || key.delete) {
        setValues((v) => ({ ...v, [field.name]: (v[field.name] ?? "").slice(0, -1) }));
      } else if (field.kind.t === "bool" && input === " ") {
        setValues((v) => ({ ...v, [field.name]: v[field.name] === "true" ? "false" : "true" }));
      } else if (input && !key.ctrl) {
        setValues((v) => ({ ...v, [field.name]: (v[field.name] ?? "") + input }));
      }
    },
    { isActive: active },
  );

  const label = Math.min(
    16,
    fields.reduce((w, f) => Math.max(w, f.label.length), 0),
  );
  return (
    <Box width={width} flexDirection="column">
      {fields.map((f, i) => (
        <Text key={f.name} color={active && i === at ? "cyan" : undefined}>
          <Text dimColor>{active && i === at ? "❯ " : "  "}</Text>
          {clip(f.label, label).padEnd(label, " ")}
          <Text dimColor>{f.required ? " * " : "   "}</Text>
          {clip(shown(f, values[f.name] ?? ""), Math.max(1, width - label - 6))}
        </Text>
      ))}
      <Text dimColor>
        {clip(`[${node.kind.submit}] ${complete(fields, values) ? "enter" : "· required fields empty"}`, width)}
      </Text>
    </Box>
  );
}
