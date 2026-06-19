import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  Input,
  OnDestroy,
  signal,
  viewChild,
} from "@angular/core";
import { IconComponent } from "../../shared/icon.component";
import { ReviewStore } from "../../agents/review.store";
import { isBlock } from "../../agents/review.store";
import { Row } from "./unified-diff";

function refLines(c: { fromLine: number; toLine: number }): string {
  return c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`;
}

function rangeLabel(range: { fromIdx: number; toIdx: number }, rows: Row[]): string {
  const block = rows
    .slice(range.fromIdx, range.toIdx + 1)
    .filter((r): r is Extract<Row, { type: "code" }> => r.type === "code");
  if (!block.length) return "";
  const a = block[0].n;
  const b = block[block.length - 1].n;
  return a === b ? `line ${a}` : `lines ${a}–${b}`;
}

interface DragState {
  anchor: number;
  current: number;
}

interface ComposerState {
  fromIdx: number;
  toIdx: number;
}

/**
 * Unified inline-review code renderer.
 * Renders diff or file rows with a comment gutter, drag-range select,
 * inline composer, and saved-comment cards.
 *
 * NOTE: Inputs use decorator @Input backed by signals (same pattern as
 * RuntimeRowComponent) so the component is testable under vitest JIT.
 * `componentRef.setInput()` routes to decorator inputs correctly.
 */
@Component({
  selector: "app-review-code",
  changeDetection: ChangeDetectionStrategy.OnPush,
  standalone: true,
  imports: [IconComponent],
  template: `
    <div
      class="scroll-y"
      style="flex:1;background:var(--bg);min-height:0"
      [style.user-select]="drag() ? 'none' : 'auto'"
    >
      <div style="position:relative">
        @for (r of rows(); track $index; let i = $index) {
          @if (r.type === 'hunk') {
            <div [style.border-top]="i ? '1px solid var(--hair)' : 'none'" style="display:flex;align-items:center;padding:5px 14px 5px 38px;font-size:11px;color:var(--accent-2);background:color-mix(in oklch,var(--accent-2),transparent 93%)">
              <span class="tnum" style="opacity:0.95">{{ r.meta }}</span>
            </div>
          } @else {
            <!-- code row -->
            <div
              [attr.data-rowkind]="'code'"
              (mouseenter)="onRowEnter($index)"
              (mouseleave)="onRowLeave($index)"
              style="display:flex;font-size:12px;line-height:1.7"
              [style.background]="rowBg($index, r.k)"
              [style.box-shadow]="commentCovering($index) ? 'inset 2px 0 0 var(--accent)' : 'none'"
            >
              <!-- gutter: hover +, drag handle, saved marker -->
              <span
                style="width:24px;flex:none;display:flex;align-items:center;justify-content:center;user-select:none"
                [style.background]="inSelection($index) ? 'color-mix(in oklch,var(--accent),transparent 70%)' : 'transparent'"
                [style.cursor]="showPlus($index) ? 'pointer' : 'default'"
              >
                @if (commentCovering($index); as cov) {
                  @if (cov.fromIdx === $index) {
                    <!-- anchor chip -->
                    <span title="comment saved" style="width:15px;height:15px;border-radius:4px;display:grid;place-items:center;background:var(--accent);color:#06070b">
                      <app-icon name="chat" [px]="10" />
                    </span>
                  } @else {
                    <!-- range dot -->
                    <span style="width:5px;height:5px;border-radius:50%;background:color-mix(in oklch,var(--accent),transparent 30%)"></span>
                  }
                } @else if (showPlus($index)) {
                  <button
                    data-plus
                    title="Comment on this line — or drag down to select a range"
                    (mousedown)="startDrag($index, $event)"
                    style="width:16px;height:16px;border-radius:4px;border:none;display:grid;place-items:center;cursor:grab;background:var(--accent);color:#06070b;padding:0"
                  >
                    <app-icon name="plus" [px]="11" />
                  </button>
                }
              </span>

              <!-- line number -->
              <span class="tnum" style="width:40px;flex:none;text-align:right;padding:0 10px 0 0;color:var(--ink-4);user-select:none">{{ r.n }}</span>

              <!-- +/- marker (diff view only) -->
              @if (view() === 'diff') {
                <span style="width:16px;flex:none;text-align:center;user-select:none"
                  [style.color]="r.k === '+' ? 'var(--code-add-ink)' : r.k === '-' ? 'var(--code-del-ink)' : 'var(--ink-4)'">{{ r.k === ' ' ? '' : r.k }}</span>
              }

              <!-- code text (v1: plain) -->
              <span
                style="flex:1;white-space:pre-wrap;word-break:break-word;padding-right:14px"
                [style.padding-left]="view() === 'diff' ? '0' : '14px'"
                [style.color]="r.k === '+' ? 'var(--code-add-ink)' : r.k === '-' ? 'var(--code-del-ink)' : 'var(--ink-2)'"
              >{{ r.s || ' ' }}</span>
            </div>

            <!-- saved comment cards under the last row of each comment -->
            @for (c of commentsEndingAt($index); track c.id) {
              <div style="display:flex;background:var(--panel);border-top:1px solid var(--hair);border-bottom:1px solid var(--hair)">
                <span style="width:24px;flex:none;background:color-mix(in oklch,var(--accent),transparent 86%);box-shadow:inset 2px 0 0 var(--accent)"></span>
                <div style="flex:1;padding:9px 14px 9px 12px;min-width:0">
                  <div style="display:flex;align-items:center;gap:7px">
                    <span style="width:17px;height:17px;flex:none;border-radius:50%;display:grid;place-items:center;font-size:8.5px;font-weight:700;color:var(--accent);background:color-mix(in oklch,var(--accent),transparent 84%);border:1px solid color-mix(in oklch,var(--accent),transparent 60%)">YOU</span>
                    <span style="font-size:11px;color:var(--ink-2)">You</span>
                    <span class="tnum" style="font-size:9.5px;color:var(--ink-4)">· {{ isBlockComment(c) ? 'lines ' + refLinesOf(c) : 'line ' + c.fromLine }}</span>
                    <span class="chip" style="font-size:8.5px;padding:0 5px;color:var(--accent);border-color:color-mix(in oklch,var(--accent),transparent 60%)">pending</span>
                    <button
                      (click)="removeComment(c.id)"
                      title="Delete comment"
                      style="margin-left:auto;background:transparent;border:none;color:var(--ink-4);cursor:pointer;display:flex;padding:var(--sp-1);border-radius:3px"
                    >
                      <app-icon name="trash" size="sm" />
                    </button>
                  </div>
                  <div style="font-size:12px;color:var(--ink);line-height:1.5;margin-top:5px;white-space:pre-wrap;word-break:break-word">{{ c.note }}</div>
                </div>
              </div>
            }

            <!-- inline composer under the range's last row -->
            @if (composer() && composer()!.toIdx === $index) {
              <div style="display:flex;background:color-mix(in oklch,var(--accent),transparent 95%);border-top:1px solid color-mix(in oklch,var(--accent),transparent 70%);border-bottom:1px solid var(--hair)">
                <span style="width:24px;flex:none;background:color-mix(in oklch,var(--accent),transparent 70%)"></span>
                <div style="flex:1;padding:10px 14px 10px 12px">
                  <div style="display:flex;align-items:center;gap:7px;margin-bottom:7px">
                    <app-icon name="chat" size="sm" [color]="'var(--accent)'" />
                    <span style="font-size:10.5px;color:var(--ink-2)">Comment on <b style="color:var(--ink)">{{ composerLabel() }}</b></span>
                    <span class="up" style="margin-left:auto;font-size:8.5px;color:var(--ink-4)">review · queued for agent</span>
                  </div>
                  <textarea
                    #ta
                    [value]="draft()"
                    rows="3"
                    placeholder="Leave a note for the agent — what to change and why…"
                    (input)="onDraftInput($event)"
                    (keydown)="onComposerKeydown($event)"
                    style="width:100%;resize:vertical;min-height:56px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);padding:8px 10px;color:var(--ink);font-family:var(--font-mono);font-size:12px;line-height:1.55;outline:none"
                  ></textarea>
                  <div style="display:flex;align-items:center;gap:8px;margin-top:8px">
                    <span style="font-size:9.5px;color:var(--ink-4)"><span class="kbd">⌘</span> <span class="kbd">↵</span> to save</span>
                    <div style="margin-left:auto;display:flex;gap:7px">
                      <button class="btn ghost-hair" (click)="cancel()" style="padding:4px 12px;font-size:11px">Cancel</button>
                      <button class="btn primary" (click)="save()" [disabled]="!draft().trim()" style="padding:4px 12px;font-size:11px">Save</button>
                    </div>
                  </div>
                </div>
              </div>
            }
          }
        }
      </div>
    </div>
  `,
})
export class ReviewCodeComponent implements OnDestroy {
  readonly review = inject(ReviewStore);

  // Inputs: decorator @Input backed by signals for vitest JIT compatibility.
  // (signal `input.required()` triggers NG0950 under raw JIT before all inputs
  // are set; decorator @Input + backing signal is the repo's established pattern.)
  readonly agent = signal("");
  readonly file = signal("");
  readonly view = signal<"diff" | "file">("diff");
  readonly rows = signal<Row[]>([]);
  readonly lang = signal("");

  @Input("agent") set agentInput(v: string) { this.agent.set(v); }
  @Input("file") set fileInput(v: string) { this.file.set(v); }
  @Input("view") set viewInput(v: "diff" | "file") { this.view.set(v); }
  @Input("rows") set rowsInput(v: Row[]) { this.rows.set(v); }
  @Input("lang") set langInput(v: string) { this.lang.set(v); }

  // State signals
  readonly hover = signal(-1);
  readonly drag = signal<DragState | null>(null);
  readonly composer = signal<ComposerState | null>(null);
  readonly draft = signal("");

  // textarea ref for auto-focus
  readonly ta = viewChild<ElementRef<HTMLTextAreaElement>>("ta");

  // Filtered comments for this file + view
  readonly comments = computed(() =>
    this.review.list(this.agent()).filter(
      (c) => c.file === this.file() && c.view === this.view()
    )
  );

  // Computed label for the composer header
  readonly composerLabel = computed(() => {
    const c = this.composer();
    if (!c) return "";
    return rangeLabel(c, this.rows());
  });

  // Window mouseup handler reference (for cleanup)
  private mouseupHandler: (() => void) | null = null;

  constructor() {
    // Register window mouseup after first render
    afterNextRender(() => {
      this._installMouseup();
    });
  }

  private _installMouseup(): void {
    if (this.mouseupHandler) return;
    this.mouseupHandler = () => {
      const d = this.drag();
      if (d) {
        const a = Math.min(d.anchor, d.current);
        const b = Math.max(d.anchor, d.current);
        this.openComposer(a, b);
      }
      this.drag.set(null);
    };
    window.addEventListener("mouseup", this.mouseupHandler);
  }

  ngOnDestroy(): void {
    if (this.mouseupHandler) {
      window.removeEventListener("mouseup", this.mouseupHandler);
      this.mouseupHandler = null;
    }
  }

  // ----- Row helpers -----

  rowBg(i: number, k: string): string {
    if (this.inSelection(i) || this.inComposer(i)) {
      return "color-mix(in oklch,var(--accent),transparent 84%)";
    }
    if (k === "+") return "var(--code-add-bg)";
    if (k === "-") return "var(--code-del-bg)";
    return "transparent";
  }

  commentCovering(i: number) {
    return this.comments().find((c) => i >= c.fromIdx && i <= c.toIdx) ?? null;
  }

  commentsEndingAt(i: number) {
    return this.comments().filter((c) => c.toIdx === i);
  }

  inSelection(i: number): boolean {
    const d = this.drag();
    return !!d && i >= Math.min(d.anchor, d.current) && i <= Math.max(d.anchor, d.current);
  }

  inComposer(i: number): boolean {
    const c = this.composer();
    return !!c && i >= c.fromIdx && i <= c.toIdx;
  }

  showPlus(i: number): boolean {
    return (
      this.hover() === i &&
      !this.composer() &&
      !this.drag() &&
      !this.commentCovering(i)
    );
  }

  // ----- Interaction -----

  onRowEnter(i: number): void {
    this.hover.set(i);
    const d = this.drag();
    if (d) {
      this.drag.set({ ...d, current: i });
    }
  }

  onRowLeave(i: number): void {
    if (this.hover() === i) {
      this.hover.set(-1);
    }
  }

  startDrag(i: number, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    if (this.composer()) return;
    this.drag.set({ anchor: i, current: i });
    // Ensure mouseup handler is installed (may not have run afterNextRender in tests)
    this._installMouseup();
  }

  openComposer(a: number, b: number): void {
    this.composer.set({ fromIdx: a, toIdx: b });
    this.draft.set("");
    // Focus textarea after DOM updates
    setTimeout(() => {
      const taEl = this.ta();
      if (taEl?.nativeElement) {
        taEl.nativeElement.focus();
      }
    }, 0);
  }

  cancel(): void {
    this.composer.set(null);
    this.draft.set("");
  }

  save(): void {
    const c = this.composer();
    const note = this.draft().trim();
    if (!c || !note) return;

    const block = this.rows()
      .slice(c.fromIdx, c.toIdx + 1)
      .filter((r): r is Extract<Row, { type: "code" }> => r.type === "code");

    const fromLine = block[0]?.n ?? null;
    const toLine = block[block.length - 1]?.n ?? fromLine;

    this.review.add(this.agent(), {
      file: this.file(),
      view: this.view(),
      lang: this.lang(),
      fromIdx: c.fromIdx,
      toIdx: c.toIdx,
      fromLine: fromLine!,
      toLine: toLine!,
      side: block[0]?.side ?? "new",
      snippet: (block[0]?.s ?? "").trim(),
      lines: block.map((r) => r.s),
      note,
    });

    this.cancel();
  }

  onDraftInput(e: Event): void {
    this.draft.set((e.target as HTMLTextAreaElement).value);
  }

  onComposerKeydown(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      this.save();
    }
    if (e.key === "Escape") {
      this.cancel();
    }
  }

  // ----- SavedCommentCard helpers -----

  isBlockComment(c: { fromIdx: number; toIdx: number }): boolean {
    return isBlock(c);
  }

  refLinesOf(c: { fromLine: number; toLine: number }): string {
    return refLines(c);
  }

  removeComment(id: string): void {
    this.review.remove(this.agent(), id);
  }
}
