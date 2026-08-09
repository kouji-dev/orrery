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
import type * as monacoApi from "monaco-editor";

import { ReviewStore } from "../../agents/review.store";
import { EditorNavService } from "../../commands/editor-nav.service";
import { UiStore } from "../../ui/ui.store";
import { registerEditor } from "../editor-cap";
import { applyMonacoTheme, loadMonaco, MonacoApi, monacoLanguage } from "../monaco-loader";
import { DiffStats, lineChangeStats } from "./chunk-stats";
import { attachReviewComments, MonacoReviewApi } from "./review-comments.monaco";

/**
 * Unified inline-review code surface for the agent diff view + file view —
 * Monaco edition (B1.1 migration; formerly CodeMirror `unifiedMergeView`).
 *
 * `view="diff"` renders a Monaco DiffEditor in INLINE mode (old text folded in
 * as removed lines, unchanged regions collapsed) and `view="file"` a plain
 * read-only editor. The review-comment UX (hover +, drag range, composer,
 * cards) attaches to the modified/only editor — new-side line anchors, exactly
 * as before. Stats come from the diff editor's own line changes.
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
      /* fill the pane so the editor's own scroller gets ALL available height */
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
      :host ::ng-deep .monaco-editor,
      :host ::ng-deep .monaco-editor .margin,
      :host ::ng-deep .monaco-editor-background {
        background-color: var(--bg) !important;
      }
      /* flat row tints matching the app's diff palette */
      :host ::ng-deep .monaco-editor .line-insert,
      :host ::ng-deep .monaco-editor .char-insert {
        background-color: var(--code-add-bg) !important;
      }
      :host ::ng-deep .monaco-editor .line-delete,
      :host ::ng-deep .monaco-editor .char-delete {
        background-color: var(--code-del-bg) !important;
      }
    `,
  ],
})
export class UnifiedCodeComponent implements OnDestroy {
  private readonly review = inject(ReviewStore);
  private readonly ui = inject(UiStore);
  private readonly editorNav = inject(EditorNavService);

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

  /** Exact header stats from the diff editor's own line changes (null for view="file"). */
  @Output() readonly stats = new EventEmitter<DiffStats | null>();

  private readonly host = viewChild.required<ElementRef<HTMLElement>>("host");
  private monaco: MonacoApi | null = null;
  private editor: monacoApi.editor.IStandaloneCodeEditor | null = null;
  private diffEditor: monacoApi.editor.IStandaloneDiffEditor | null = null;
  private models: monacoApi.editor.ITextModel[] = [];
  private subs: monacoApi.IDisposable[] = [];
  private api: MonacoReviewApi | null = null;
  /** Unhooks this component's entry in the global editor cap (A0.6). */
  private unregisterCap: (() => void) | null = null;
  /** Bumped when a new editor is live, so dependent effects re-push. */
  private readonly viewGen = signal(0);
  private renderToken = 0;

  constructor() {
    // Rebuild when content, language, or view mode changes (theme is global).
    effect(() => {
      void this.render(this.oldText(), this.newText(), this.view(), this.lang());
    });
    // Theme switch restyles every live Monaco editor in place.
    effect(() => {
      const theme = this.ui.tweaks().theme;
      if (this.monaco) applyMonacoTheme(this.monaco, theme);
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
      this.api?.setComments(comments);
    });
    // Go-to-line / open-at-line (B2.3): consume a posted nav target once the
    // editor for that agent+file is live.
    effect(() => {
      const t = this.editorNav.target();
      this.viewGen();
      const editor = this.activeEditor();
      const model = editor?.getModel() as monacoApi.editor.ITextModel | null | undefined;
      if (!t || !editor || !model) return;
      if (t.agentId !== this.agent() || t.file !== this.file()) return;
      const line = Math.max(1, Math.min(t.line, model.getLineCount()));
      editor.setPosition({ lineNumber: line, column: Math.max(1, t.col) });
      editor.revealLineInCenter(line);
      editor.focus();
      this.editorNav.consume(t);
    });
  }

  ngOnDestroy(): void {
    this.teardown();
  }

  /** The editor review comments + nav act on: modified side (diff) or the only one. */
  private activeEditor(): monacoApi.editor.ICodeEditor | null {
    return this.diffEditor ? this.diffEditor.getModifiedEditor() : this.editor;
  }

  private teardown(): void {
    this.unregisterCap?.();
    this.unregisterCap = null;
    this.api?.dispose();
    this.api = null;
    for (const s of this.subs) s.dispose();
    this.subs = [];
    this.editor?.dispose();
    this.editor = null;
    this.diffEditor?.dispose();
    this.diffEditor = null;
    for (const m of this.models) m.dispose();
    this.models = [];
  }

  private async render(
    oldText: string,
    newText: string,
    mode: "diff" | "file",
    lang: string,
  ): Promise<void> {
    const token = ++this.renderToken;
    const el = this.host().nativeElement;
    this.teardown();
    el.textContent = "loading…";

    let monaco: MonacoApi;
    let langId: string;
    try {
      monaco = await loadMonaco();
      langId = await monacoLanguage(lang);
    } catch {
      if (token === this.renderToken) el.textContent = newText;
      return;
    }
    if (token !== this.renderToken) return;
    this.monaco = monaco;
    applyMonacoTheme(monaco, this.ui.tweaks().theme);

    // Yield a macrotask so the triggering frame paints before the synchronous
    // editor build (same rationale as the CM implementation this replaces).
    await new Promise<void>((r) => setTimeout(r, 0));
    if (token !== this.renderToken) return;
    el.textContent = "";

    try {
      const common: monacoApi.editor.IEditorConstructionOptions = {
        readOnly: true,
        glyphMargin: true, // review-comment hover + / anchors
        wordWrap: "on",
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        fontSize: parseFloat(getComputedStyle(el).fontSize) || 12,
        fixedOverflowWidgets: true,
        renderLineHighlight: "none",
        stickyScroll: { enabled: false },
      };
      let capTarget: { dispose(): void };
      if (mode === "diff") {
        const diff = monaco.editor.createDiffEditor(el, {
          ...common,
          renderSideBySide: false, // unified/inline — deletions fold in as rows
          hideUnchangedRegions: { enabled: true, contextLineCount: 3, minimumLineCount: 4 },
          renderOverviewRuler: false,
        });
        const original = monaco.editor.createModel(oldText, langId);
        const modified = monaco.editor.createModel(newText, langId);
        diff.setModel({ original, modified });
        this.models = [original, modified];
        this.diffEditor = diff;
        capTarget = diff;
        // The diff computes async in the worker; line changes land afterwards.
        this.subs.push(
          diff.onDidUpdateDiff(() => {
            if (this.diffEditor !== diff) return;
            this.stats.emit(lineChangeStats(diff.getLineChanges() ?? [], oldText, newText));
          }),
        );
        this.api = attachReviewComments(monaco, diff.getModifiedEditor(), this.commentHost());
      } else {
        const model = monaco.editor.createModel(newText, langId);
        const editor = monaco.editor.create(el, { ...common, model });
        this.models = [model];
        this.editor = editor;
        capTarget = editor;
        this.stats.emit(null);
        this.api = attachReviewComments(monaco, editor, this.commentHost());
      }
      // A0.6 editor cap: demote to plain text to bound the webview heap.
      const mine = capTarget;
      this.unregisterCap = registerEditor(() => {
        if ((this.diffEditor ?? this.editor) !== mine) return; // superseded
        this.teardown();
        el.textContent = newText;
      });
      this.viewGen.update((n) => n + 1);
    } catch (e) {
      console.warn("[unified-code] editor build failed, showing plain text", e);
      el.textContent = newText;
    }
  }

  private commentHost(): { save(f: number, t: number, n: string): void; remove(id: string): void } {
    return {
      save: (fromLine, toLine, note) => this.saveComment(fromLine, toLine, note),
      remove: (id) => this.review.remove(this.agent(), id),
    };
  }

  private saveComment(fromLine: number, toLine: number, note: string): void {
    const model = this.activeEditor()?.getModel() as monacoApi.editor.ITextModel | null | undefined;
    if (!model) return;
    const lines = model.getLineCount();
    const from = Math.max(1, Math.min(fromLine, lines));
    const to = Math.max(from, Math.min(toLine, lines));
    const text: string[] = [];
    for (let n = from; n <= to; n++) text.push(model.getLineContent(n));
    this.review.add(this.agent(), {
      file: this.file(),
      view: this.view(),
      lang: this.lang(),
      fromLine: from,
      toLine: to,
      side: this.view() === "diff" ? "new" : "file",
      snippet: (text[0] ?? "").trim(),
      lines: text,
      note,
    });
  }
}
