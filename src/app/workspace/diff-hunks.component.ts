import {
  ChangeDetectionStrategy,
  Component,
  input,
} from "@angular/core";
import { DiffHunk, DiffLine } from "../models";

/**
 * Shared hunk renderer — extracted from diff-view so every git surface
 * (commit diff, range diff, blame side-panel) uses ONE renderer.
 *
 * Matches the HunkRows design (repo-diff.jsx 59-84):
 *   - hunk-meta band (accent-2 tinted, top border separating hunks)
 *   - line-number gutter (44px, right-aligned, tnum, ink-4)
 *   - +/- col (16px, centre-aligned, tinted ink)
 *   - code col (flex-1, pre-wrap)
 *
 * Phase-2 extension points (accepted but unused for now):
 *   - `lead`  — optional { hunkHead, lineHead } render callbacks
 *   - `lineStyle` — optional (hunk, line, index) → CSS background override
 *
 * All layout lengths use density tokens (`--sp-*`, `--fs-*`) — NO hardcoded px.
 */
@Component({
  selector: "app-diff-hunks",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (hunk of hunks(); track hunk.meta; let hi = $index) {
      <!-- hunk-meta band -->
      <div
        class="hunk-meta tnum"
        [style.border-top]="hi > 0 ? '1px solid var(--hair)' : 'none'"
      >
        <!-- Phase-2: lead.hunkHead slot (wired in Phase 2) -->
        <span style="opacity: 0.95">{{ hunk.meta }}</span>
      </div>

      <!-- diff lines -->
      @for (ln of hunk.lines; track ln.n; let li = $index) {
        <div
          class="diff-line"
          [style.background]="resolveLineBg(hunk, ln, li)"
        >
          <!-- Phase-2: lead.lineHead slot -->
          <span class="ln-num tnum">{{ ln.n }}</span>
          <span
            class="ln-kind"
            [style.color]="kindInk(ln.k)"
          >{{ ln.k }}</span>
          <span
            class="ln-code"
            [style.color]="kindInk(ln.k)"
          >{{ ln.s || ' ' }}</span>
        </div>
      }
    }
  `,
  styles: [
    `
      :host {
        display: block;
        font-family: var(--font-mono, monospace);
      }

      /* hunk-meta band */
      .hunk-meta {
        display: flex;
        align-items: center;
        padding: var(--sp-2) var(--sp-6);
        font-size: var(--fs-xs);
        color: var(--accent-2);
        background: color-mix(in oklch, var(--accent-2), transparent 93%);
      }

      /* one diff line */
      .diff-line {
        display: flex;
        font-size: var(--fs-ui);
        line-height: 1.7;
      }

      /* line-number gutter */
      .ln-num {
        width: var(--sp-11, 44px);
        flex: none;
        text-align: right;
        padding: 0 var(--sp-3) 0 0;
        color: var(--ink-4);
        user-select: none;
      }

      /* +/- kind column */
      .ln-kind {
        width: var(--sp-5, 16px);
        flex: none;
        text-align: center;
        user-select: none;
      }

      /* code column */
      .ln-code {
        flex: 1;
        white-space: pre-wrap;
        word-break: break-word;
        padding-right: var(--sp-6);
      }
    `,
  ],
})
export class DiffHunksComponent {
  /** The hunks to render. */
  readonly hunks = input.required<DiffHunk[]>();
  /** Language tag (unused in template — available to Phase-2 consumers). */
  readonly lang = input<string>("");

  /**
   * Phase-2 lead render callbacks. Defined now so components can bind them;
   * the template does not yet render their output (Phase 2 fills that in).
   */
  readonly lead = input<{
    hunkHead?: (hunk: DiffHunk, index: number) => unknown;
    lineHead?: (hunk: DiffHunk, line: DiffLine, index: number) => unknown;
  } | null>(null);

  /**
   * Phase-2 per-line background override. When provided, supersedes the
   * default +/- tinting for that line.
   */
  readonly lineStyle = input<
    ((hunk: DiffHunk, line: DiffLine, index: number) => string) | null
  >(null);

  resolveLineBg(hunk: DiffHunk, ln: DiffLine, li: number): string {
    const custom = this.lineStyle();
    if (custom) return custom(hunk, ln, li);
    switch (ln.k) {
      case "+": return "var(--code-add-bg)";
      case "-": return "var(--code-del-bg)";
      default:  return "transparent";
    }
  }

  kindInk(k: "+" | "-" | " "): string {
    switch (k) {
      case "+": return "var(--code-add-ink)";
      case "-": return "var(--code-del-ink)";
      default:  return "var(--ink-2)";
    }
  }
}
