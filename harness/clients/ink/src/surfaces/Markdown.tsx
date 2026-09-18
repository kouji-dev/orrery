/**
 * `markdown` — the streaming one.
 *
 * Open question 1, decided: the markdown renderer is hand-rolled. `marked` and
 * friends exist to produce HTML, carry a parser this client would ship without
 * using, and would have to be taught the one rule that matters here — that a
 * half-arrived document must not be formatted. The subset a terminal can show
 * (headings, emphasis, code spans, fenced code, lists, rules, quotes) is a
 * hundred lines, and those hundred lines are testable.
 *
 * **Plain until complete.** While `complete` is false the body is drawn as
 * plain text. A fence that has not closed yet would otherwise flip the rest of
 * the answer into code styling and then flip it back, which reads as a bug.
 */

import { Box, Text } from "ink";
import type { ReactElement } from "react";

import type { SurfaceProps } from "./kinds.js";

/** One styled run inside a line. */
export interface Span {
  text: string;
  bold?: boolean;
  italic?: boolean;
  code?: boolean;
}

/** One line of a rendered document. */
export interface Line {
  spans: Span[];
  /** Fenced code: drawn whole, never re-parsed for emphasis. */
  code?: boolean;
  heading?: boolean;
  quote?: boolean;
}

/** Split a line into emphasis runs. Deliberately small and greedy-free. */
export function spans(text: string): Span[] {
  const out: Span[] = [];
  let buffer = "";
  const flush = (): void => {
    if (buffer) out.push({ text: buffer });
    buffer = "";
  };
  for (let i = 0; i < text.length; ) {
    const rest = text.slice(i);
    const code = /^`([^`]+)`/.exec(rest);
    if (code) {
      flush();
      out.push({ text: code[1]!, code: true });
      i += code[0].length;
      continue;
    }
    const bold = /^\*\*([^*]+)\*\*/.exec(rest);
    if (bold) {
      flush();
      out.push({ text: bold[1]!, bold: true });
      i += bold[0].length;
      continue;
    }
    const italic = /^(?:\*([^*]+)\*|_([^_]+)_)/.exec(rest);
    if (italic) {
      flush();
      out.push({ text: (italic[1] ?? italic[2])!, italic: true });
      i += italic[0].length;
      continue;
    }
    buffer += text[i];
    i += 1;
  }
  flush();
  return out.length > 0 ? out : [{ text: "" }];
}

/** Turn a markdown document into lines a terminal can draw. */
export function parse(source: string): Line[] {
  const lines: Line[] = [];
  let fenced = false;
  for (const raw of source.replace(/\n+$/, "").split("\n")) {
    if (/^\s*```/.test(raw)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) {
      lines.push({ spans: [{ text: raw }], code: true });
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(raw);
    if (heading) {
      lines.push({ spans: [{ text: heading[2]!, bold: true }], heading: true });
      continue;
    }
    if (/^\s*([-*_])\1{2,}\s*$/.test(raw)) {
      lines.push({ spans: [{ text: "─".repeat(8) }] });
      continue;
    }
    const quote = /^>\s?(.*)$/.exec(raw);
    if (quote) {
      lines.push({ spans: spans(quote[1]!), quote: true });
      continue;
    }
    const bullet = /^(\s*)[-*+]\s+(.*)$/.exec(raw);
    if (bullet) {
      lines.push({ spans: [{ text: `${bullet[1]}• ` }, ...spans(bullet[2]!)] });
      continue;
    }
    const numbered = /^(\s*)(\d+)\.\s+(.*)$/.exec(raw);
    if (numbered) {
      lines.push({ spans: [{ text: `${numbered[1]}${numbered[2]}. ` }, ...spans(numbered[3]!)] });
      continue;
    }
    lines.push({ spans: spans(raw) });
  }
  return lines;
}

/** Draw a markdown surface. */
export function MarkdownSurface({ node, width }: SurfaceProps<"markdown">): ReactElement {
  const { value, complete } = node.kind;
  if (!complete) {
    // Half-arrived: plain, and visibly still arriving.
    return (
      <Box width={width} flexDirection="column">
        <Text wrap="wrap">{value.replace(/\n+$/, "")}</Text>
      </Box>
    );
  }
  return (
    <Box width={width} flexDirection="column">
      {parse(value).map((line, i) => (
        <Text
          key={i}
          wrap="wrap"
          bold={line.heading === true}
          dimColor={line.quote === true}
          color={line.code === true ? "cyan" : undefined}
        >
          {line.code === true
            ? `  ${line.spans[0]?.text ?? ""}`
            : line.spans.map((span, j) => (
                <Text
                  key={j}
                  bold={span.bold === true}
                  italic={span.italic === true}
                  color={span.code === true ? "cyan" : undefined}
                >
                  {span.text}
                </Text>
              ))}
        </Text>
      ))}
    </Box>
  );
}
