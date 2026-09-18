/**
 * `table` — columns and rows, already formatted by whoever knew the units.
 *
 * Degradation: below four columns of room per column, the table becomes one
 * `label: value` block per row. A table that overflows is unreadable; a list
 * is merely long.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";
import { pad, styleProps } from "../theme.js";

/** How wide each column wants to be, before the budget is applied. */
export function widths(columns: string[], rows: Array<Array<{ text: string }>>): number[] {
  return columns.map((name, i) =>
    rows.reduce((wide, row) => Math.max(wide, (row[i]?.text ?? "").length), name.length),
  );
}

/** Shrink the widest column until the row fits. */
export function fit(wanted: number[], budget: number): number[] {
  const gaps = Math.max(0, wanted.length - 1);
  const out = [...wanted];
  let total = out.reduce((a, b) => a + b, 0) + gaps;
  while (total > budget) {
    const widest = out.indexOf(Math.max(...out));
    if ((out[widest] ?? 0) <= 3) break;
    out[widest] = (out[widest] ?? 0) - 1;
    total -= 1;
  }
  return out;
}

/** Draw a table surface. */
export function TableSurface({ node, width }: SurfaceProps<"table">): ReactElement {
  const { columns, rows = [] } = node.kind;
  const wanted = widths(columns, rows);
  const room = fit(wanted, width);
  const needed = room.reduce((a, b) => a + b, 0) + Math.max(0, room.length - 1);

  if (needed > width || room.some((w) => w < 4)) {
    // Too narrow for a grid: one block per row, which never overflows.
    return (
      <Box width={width} flexDirection="column">
        {rows.map((row, i) => (
          <Box key={i} flexDirection="column" marginBottom={rows.length > 1 ? 1 : 0}>
            {columns.map((name, j) => (
              <Text key={name} wrap="truncate-end">
                <Text dimColor>{name}: </Text>
                <Text {...styleProps(row[j]?.style)}>{row[j]?.text ?? ""}</Text>
              </Text>
            ))}
          </Box>
        ))}
      </Box>
    );
  }

  return (
    <Box width={width} flexDirection="column">
      <Text dimColor bold>
        {columns.map((name, i) => pad(name, room[i] ?? 0)).join(" ")}
      </Text>
      {rows.map((row, i) => (
        <Text key={i}>
          {columns.map((_, j) => (
            <Text key={j} {...styleProps(row[j]?.style)}>
              {pad(row[j]?.text ?? "", room[j] ?? 0)}
              {j < columns.length - 1 ? " " : ""}
            </Text>
          ))}
        </Text>
      ))}
    </Box>
  );
}
