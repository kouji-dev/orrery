import {
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  EventEmitter,
  inject,
  Input,
  OnDestroy,
  Output,
  signal,
  viewChild,
} from "@angular/core";
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { ReviewStore } from "../../agents/review.store";
import { UiStore } from "../../ui/ui.store";
import { buildTheme, CMCore, loadCMCore, loadLangExt } from "../code-lang";
import { chunkStats, DiffStats } from "./chunk-stats";
import { reviewCommentsExt, ReviewCommentsApi } from "./review-comments.ext";

/**
 * Unified inline-review code surface for the agent diff view + file view.
 *
 * Renders the CURRENT (new-side) content in a read-only CodeMirror editor;
 * `view="diff"` adds `unifiedMergeView` (Myers diff against `oldText`,
 * virtualized scrolling, deleted chunks as widgets) and `view="file"` is the
 * same editor without the merge layer. The review-comment UX (hover +, drag
 * range, composer, cards) is a CM extension — see review-comments.ext.ts.
 *
 * NOTE: Inputs use decorator @Input backed by signals (repo pattern) so the
 * component is testable under vitest JIT.
 */
@Component({
  selector: "app-unified-code",
  changeDetection: ChangeDetectionStrategy.OnPush,
  standalone: true,
  template: `<div #host class="code-host"></div>`,
  styles: [
    `
      /* fill the pane so the editor's own scroller gets ALL available height —
         the height chain here is what keeps large diffs scrollable */
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        height: 100%;
        min-height: 0;
      }
      .code-host {
        flex: 1;
        min-height: 0;
        overflow: hidden;
        font-size: var(--fs-ui);
      }
      :host ::ng-deep .cm-editor {
        height: 100% !important;
        background-color: var(--bg) !important;
      }
      :host ::ng-deep .cm-scroller {
        height: 100%;
        overflow: auto;
      }
      :host ::ng-deep .cm-content {
        min-height: 100% !important;
      }
      :host ::ng-deep .cm-gutters {
        min-height: 100% !important;
        background-color: var(--bg) !important;
      }
      /* ----- unified merge layer: flat row tints (mirrors code-diff) ----- */
      :host ::ng-deep .cm-changedLine {
        background-color: rgba(52, 224, 161, 0.15) !important;
      }
      :host ::ng-deep .cm-deletedChunk {
        background-color: rgba(255, 93, 122, 0.15) !important;
      }
      :host ::ng-deep .cm-insertedLine,
      :host ::ng-deep .cm-deletedLine,
      :host ::ng-deep .cm-deletedLine del {
        background: none !important;
        background-color: transparent !important;
        text-decoration: none !important;
      }
      :host ::ng-deep .cm-changedText {
        background-color: rgba(52, 224, 161, 0.34) !important;
        border-radius: 2px;
      }
      :host ::ng-deep .cm-deletedChunk .cm-deletedText {
        background-color: rgba(255, 93, 122, 0.34) !important;
        border-radius: 2px;
        text-decoration: none !important;
      }
      :host ::ng-deep .cm-collapsedLines {
        color: var(--accent-2);
        background: color-mix(in oklch, var(--accent-2), transparent 93%);
      }
    `,
  ],
})
export class UnifiedCodeComponent implements OnDestroy {
  private readonly review = inject(ReviewStore);
  private readonly ui = inject(UiStore);

  // Inputs: decorator @Input backed by signals (vitest JIT / NG0950 pattern).
  readonly agent = signal("");
  readonly file = signal("");
  readonly view = signal<"diff" | "file">("diff");
  readonly oldText = signal("");
  readonly newText = signal("");
  readonly lang = signal("");

  @Input("agent") set agentInput(v: string) { this.agent.set(v); }
  @Input("file") set fileInput(v: string) { this.file.set(v); }
  @Input("view") set viewInput(v: "diff" | "file") { this.view.set(v); }
  @Input("oldText") set oldTextInput(v: string) { this.oldText.set(v); }
  @Input("newText") set newTextInput(v: string) { this.newText.set(v); }
  @Input("lang") set langInput(v: string) { this.lang.set(v); }

  /** Exact header stats from the merge view's own diff (null for view="file"). */
  @Output() readonly stats = new EventEmitter<DiffStats | null>();

  private readonly host = viewChild.required<ElementRef<HTMLElement>>("host");
  private cmView: EditorView | null = null;
  private api: ReviewCommentsApi | null = null;
  /** Bumped when a new editor is live, so the comment-sync effect re-pushes. */
  private readonly viewGen = signal(0);
  private renderToken = 0;

  constructor() {
    // Rebuild the editor when content, language, view mode, or theme changes.
    effect(() => {
      void this.render(this.oldText(), this.newText(), this.view(), this.lang(), this.ui.tweaks().theme);
    });
    // Push comment updates into the live editor (add/remove/clear from anywhere).
    effect(() => {
      const agent = this.agent();
      const file = this.file();
      const mode = this.view();
      this.viewGen(); // re-run once a fresh editor is mounted
      const comments = this.review
        .list(agent)
        .filter((c) => c.file === file && c.view === mode)
        .map((c) => ({ id: c.id, fromLine: c.fromLine, toLine: c.toLine, note: c.note }));
      if (this.cmView && this.api) this.api.setComments(this.cmView, comments);
    });
  }

  ngOnDestroy(): void {
    this.cmView?.destroy();
    this.cmView = null;
  }

  private async render(oldText: string, newText: string, mode: "diff" | "file", lang: string, theme: "dark" | "light"): Promise<void> {
    const token = ++this.renderToken;
    const el = this.host().nativeElement;
    this.cmView?.destroy();
    this.cmView = null;
    this.api = null;
    el.textContent = "loading…";

    let cm: CMCore;
    try {
      cm = await loadCMCore();
    } catch {
      if (token === this.renderToken) el.textContent = newText;
      return;
    }
    if (token !== this.renderToken) return;

    // Yield a macrotask so the triggering frame paints before the synchronous
    // editor build (same rationale as code-diff.component.ts).
    await new Promise<void>((r) => setTimeout(r, 0));
    if (token !== this.renderToken) return;
    el.textContent = "";

    try {
      const { EditorState, Compartment } = cm.state;
      const { EditorView, lineNumbers } = cm.view;
      const langComp = new Compartment();
      const api = reviewCommentsExt(cm, {
        save: (fromLine, toLine, note) => this.saveComment(fromLine, toLine, note),
        remove: (id) => this.review.remove(this.agent(), id),
      });
      const exts: Extension[] = [
        api.extension, // first → its gutter sits leftmost, like the old layout
        lineNumbers(),
        buildTheme(cm, theme),
        EditorView.editable.of(false),
        EditorState.readOnly.of(true),
        EditorView.lineWrapping,
        langComp.of([]),
      ];
      if (mode === "diff") {
        exts.push(
          cm.merge.unifiedMergeView({
            original: oldText,
            mergeControls: false,
            gutter: true,
            highlightChanges: true,
            syntaxHighlightDeletions: true,
            collapseUnchanged: { margin: 3, minSize: 4 },
            diffConfig: { scanLimit: 500, timeout: 100 },
          }),
        );
      }
      const view = new EditorView({ doc: newText, extensions: exts, parent: el });
      this.cmView = view;
      this.api = api;
      this.viewGen.update((n) => n + 1);

      if (mode === "diff") {
        const gc = cm.merge.getChunks(view.state);
        this.stats.emit(chunkStats(gc?.chunks ?? [], cm.merge.getOriginalDoc(view.state), view.state.doc));
      } else {
        this.stats.emit(null);
      }

      if (lang) {
        void loadLangExt(lang).then((ext) => {
          if (token !== this.renderToken || this.cmView !== view) return; // stale
          view.dispatch({ effects: langComp.reconfigure(ext) });
        });
      }
    } catch (e) {
      console.warn("[unified-code] editor build failed, showing plain text", e);
      el.textContent = newText;
    }
  }

  private saveComment(fromLine: number, toLine: number, note: string): void {
    const v = this.cmView;
    if (!v) return;
    const doc = v.state.doc;
    const from = Math.max(1, Math.min(fromLine, doc.lines));
    const to = Math.max(from, Math.min(toLine, doc.lines));
    const lines: string[] = [];
    for (let n = from; n <= to; n++) lines.push(doc.line(n).text);
    this.review.add(this.agent(), {
      file: this.file(),
      view: this.view(),
      lang: this.lang(),
      fromLine: from,
      toLine: to,
      side: this.view() === "diff" ? "new" : "file",
      snippet: (lines[0] ?? "").trim(),
      lines,
      note,
    });
  }
}
