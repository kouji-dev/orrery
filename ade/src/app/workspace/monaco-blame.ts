import type * as monacoApi from "monaco-editor";
import { BlameLine } from "../models";
import { authorBucket, blameLabel, relTime } from "./review/blame-format";
import type { MonacoApi } from "./monaco-loader";

/**
 * Annotate INSIDE the editor.
 *
 * The blame used to replace the file with a hand-rolled `<pre>` surface, so
 * turning Annotate on cost the real thing: syntax highlighting, folding, find,
 * review comments, and the editor's own scrolling. Here each blame line rides
 * along as Monaco *injected text* — a `before` decoration at column 1. The
 * document is untouched (nothing is inserted into the model, so copy, save and
 * every line number stay exact), the column scrolls with the code because it
 * IS the code's line, and the editor stays the editor.
 */

/** One line's gutter cell, resolved from the blame rows. Pure data — no Monaco. */
export interface BlameSpec {
  line: number;
  /** Fixed-width label (blank on a line continuing the commit above it). */
  label: string;
  /** Injected-text class: `blm blm-h<bucket>` on a run's first line, else `blm blm-cont`. */
  cls: string;
  /** Markdown hover body: author, date, summary. */
  hover: string;
  sha: string;
}

/** Blame rows → one spec per line, run-grouped like the standalone view. */
export function blameSpecs(lines: BlameLine[], now = Date.now()): BlameSpec[] {
  return lines.map((l, i) => {
    const first = i === 0 || lines[i - 1].sha !== l.sha;
    const rel = relTime(l.when, now);
    const when = l.when ? new Date(l.when * 1000).toLocaleString() : "not committed yet";
    return {
      line: l.n,
      label: blameLabel(l, first),
      cls: first ? `blm blm-h${authorBucket(l.author)}` : "blm blm-cont",
      hover: [
        `**${l.author}**${rel ? ` · ${rel} ago` : ""}`,
        l.summary,
        `\`${l.sha ? l.sha.slice(0, 7) : "uncommitted"}\` · ${when}`,
        "",
        "_click the gutter to open this commit_",
      ].join("\n\n"),
      sha: l.sha,
    };
  });
}

/**
 * Specs → Monaco decorations. Lines past the model's end are dropped: the
 * buffer can be edited (or reloaded) while blame from the previous read is
 * still on screen, and a decoration on a line that no longer exists throws.
 */
export function blameDecorations(
  monaco: MonacoApi,
  specs: BlameSpec[],
  maxLine: number,
): monacoApi.editor.IModelDeltaDecoration[] {
  const out: monacoApi.editor.IModelDeltaDecoration[] = [];
  for (const s of specs) {
    if (s.line < 1 || s.line > maxLine) continue;
    out.push({
      range: new monaco.Range(s.line, 1, s.line, 1),
      options: {
        // The range is empty (the label is injected, not written), and Monaco
        // filters injected text on an empty range OUT of the view unless this
        // is set — `getInjectedTextInInterval` keeps only
        // `showIfCollapsed || !range.isEmpty()`. Without it the decorations
        // exist on the model and simply never render.
        showIfCollapsed: true,
        // stickiness: the label belongs to the line, not to an edit at its start
        stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
        before: {
          content: s.label,
          inlineClassName: s.cls,
          inlineClassNameAffectsLetterSpacing: true,
          cursorStops: monaco.editor.InjectedTextCursorStops.None,
        },
        hoverMessage: { value: s.hover, isTrusted: false },
      },
    });
  }
  return out;
}

/** The sha of the blame run covering `line`, for the gutter click. */
export function shaAtLine(specs: BlameSpec[], line: number): string | null {
  const s = specs.find((x) => x.line === line);
  return s && s.sha && !/^0+$/.test(s.sha) ? s.sha : null;
}
