import type { MarkedExtension, Tokens } from "marked";

/**
 * marked extension for the markdown preview: a mermaid-only fence tokenizer
 * plus the design's `.md-box` markup for code fences.
 *
 * WHY a tokenizer override: CommonMark says a closing fence may be indented at
 * most 3 spaces and an unclosed fence runs to EOF. LLM-written docs routinely
 * indent the ``` closer by 4+ spaces (nested under a list item, or plain
 * sloppiness) — the default tokenizer then swallows the REST OF THE DOCUMENT
 * into one mermaid block and the diagram fails to parse on top of it. For
 * mermaid fences only, a bare fence line closes the block wherever it sits;
 * a block with no closer ends where the next ATX heading starts (no mermaid
 * grammar has a line beginning `# `, so a heading is unambiguously the
 * document resuming), else at end of input. Every other language keeps strict
 * CommonMark: the override returns `false`, which marked treats as "fall back
 * to the default tokenizer".
 *
 * WHY no data-* attributes in the renderer output: this HTML lands through
 * Angular's [innerHTML] sanitizer, which keeps `class` but drops `data-*` and
 * `id`. Anything the post-render step needs (diagram source, theme) is set on
 * `el.dataset` by md-mermaid.ts AFTER sanitization.
 */

/** Opening line of a mermaid fence: ≤3-space indent, ``` or ~~~, `mermaid` as
 *  the first word of the info string. */
const MERMAID_OPEN = /^( {0,3})(`{3,}|~{3,})[ \t]*mermaid(?=[ \t\n]|$)[^\n]*(?:\n|$)/;
/** An ATX heading line — the one block start a mermaid body can never contain. */
const HEADING = /^ {0,3}#{1,6}(?:[ \t]|$)/;

const ESC: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };
export const escHtml = (s: string): string => s.replace(/[&<>"]/g, (c) => ESC[c]);

/** marked's indentCodeCompensation: an indented opener strips the same indent
 *  from every body line (so a fence nested under a list item reads flush). */
function dedent(body: string, indent: string): string {
  if (!indent) return body;
  return body
    .split("\n")
    .map((line) => {
      const lead = /^\s+/.exec(line)?.[0] ?? "";
      return lead.length >= indent.length ? line.slice(indent.length) : line;
    })
    .join("\n");
}

/** Tokenize a mermaid fence with the lenient closer rule. `false` = not a
 *  mermaid fence, let the default tokenizer handle it. */
function mermaidFence(src: string): Tokens.Code | false {
  const m = MERMAID_OPEN.exec(src);
  if (!m) return false;
  const [head, indent, fence] = m;
  // closer: the same fence char, at least as many, nothing else once trimmed
  const closer = new RegExp(`^${fence[0]}{${fence.length},}$`);
  const rest = src.slice(head.length);
  let at = 0;
  let bodyEnd = rest.length;
  let rawEnd = rest.length;
  while (at < rest.length) {
    const nl = rest.indexOf("\n", at);
    const lineEnd = nl === -1 ? rest.length : nl;
    const line = rest.slice(at, lineEnd);
    if (closer.test(line.trim())) {
      bodyEnd = at;
      rawEnd = nl === -1 ? rest.length : nl + 1;
      break;
    }
    if (HEADING.test(line)) {
      // unclosed block: the document resumes here — the heading is not consumed
      bodyEnd = rawEnd = at;
      break;
    }
    at = lineEnd + 1;
  }
  const text = dedent(rest.slice(0, bodyEnd).replace(/\n+$/, ""), indent);
  return { type: "code", raw: head + rest.slice(0, rawEnd), lang: "mermaid", text };
}

/** Design markup for a code token: mermaid → a `pending` diagram box that
 *  md-mermaid.ts hydrates; anything else → the `.md-box.fence` card. */
function renderCode({ text, lang, escaped, codeBlockStyle }: Tokens.Code): string {
  const l = (lang ?? "").match(/^\S*/)?.[0] ?? "";
  const body = escaped ? text : escHtml(text);
  if (l === "mermaid") {
    return `<div class="md-box diagram pending"><pre><code class="language-mermaid">${body}</code></pre></div>\n`;
  }
  const cls = l ? ` class="language-${escHtml(l)}"` : "";
  // indented code has no info string — the design draws it headerless
  const hd =
    codeBlockStyle === "indented"
      ? ""
      : `<div class="fence-hd static"><span class="chip mono">${escHtml(l || "text")}</span><span class="tnum mono">${text ? text.split("\n").length : 0} lines</span></div>`;
  return `<div class="md-box fence">${hd}<pre><code${cls}>${body}</code></pre></div>\n`;
}

export function mermaidExtension(): MarkedExtension {
  return {
    tokenizer: {
      fences(src) {
        return mermaidFence(src);
      },
    },
    renderer: {
      code(token) {
        return renderCode(token);
      },
      // tables get the design's scroll wrapper; the inner <table> is marked's own
      table(token) {
        // `this` is the live renderer; the prototype method is the untouched default
        const inner = Object.getPrototypeOf(this).table.call(this, token) as string;
        return `<div class="md-box table"><div class="box-scroll">${inner}</div></div>\n`;
      },
    },
  };
}
