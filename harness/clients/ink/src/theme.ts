/**
 * The small vocabulary of glyphs and colours every surface component shares.
 *
 * Kept in one file so two components cannot disagree about what `running`
 * looks like, and so a narrow terminal degrades in one place.
 */

import type { Status, TextStyle } from "@orrery/protocol";

/** Ink colour names. `undefined` means "the terminal's own foreground". */
export type Colour = string | undefined;

/** How a status reads at a glance. */
export function statusGlyph(status: Status | null | undefined): string {
  switch (status) {
    case "pending":
      return "·";
    case "running":
      return "▸";
    case "done":
      return "✓";
    case "failed":
      return "✗";
    case "cancelled":
      return "⊘";
    default:
      return " ";
  }
}

/** The colour a status is drawn in. */
export function statusColour(status: Status | null | undefined): Colour {
  switch (status) {
    case "running":
      return "cyan";
    case "done":
      return "green";
    case "failed":
      return "red";
    case "cancelled":
      return "yellow";
    default:
      return "gray";
  }
}

/** How a run of text is meant, as Ink props. */
export interface TextProps {
  color?: string;
  dimColor?: boolean;
  bold?: boolean;
  italic?: boolean;
}

/** `TextStyle` is a hint about meaning, never a colour; this is where it becomes one. */
export function styleProps(style: TextStyle | null | undefined): TextProps {
  switch (style) {
    case "muted":
      return { dimColor: true };
    case "emphasis":
      return { bold: true };
    case "code":
      return { color: "cyan" };
    case "error":
      return { color: "red" };
    case "success":
      return { color: "green" };
    case "warning":
      return { color: "yellow" };
    default:
      return {};
  }
}

/** Cut a string to `width` columns, with an ellipsis when it did not fit. */
export function clip(text: string, width: number): string {
  if (width <= 0) return "";
  if (text.length <= width) return text;
  if (width === 1) return "…";
  return `${text.slice(0, width - 1)}…`;
}

/** Pad to `width`, clipping first. Table columns and tree rows both want this. */
export function pad(text: string, width: number): string {
  return clip(text, width).padEnd(width, " ");
}
