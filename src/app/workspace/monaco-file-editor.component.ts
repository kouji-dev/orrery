import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  effect,
  ElementRef,
  EventEmitter,
  inject,
  Input,
  Output,
  signal,
  untracked,
  viewChild,
} from "@angular/core";
import type * as monacoApi from "monaco-editor";

import { ReviewStore } from "../agents/review.store";
import { FileHunk } from "../data-source/bridge";
import { EditorNavService } from "../commands/editor-nav.service";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { registerEditor } from "./editor-cap";
import {
  applyMonacoDensity,
  applyMonacoTheme,
  loadMonaco,
  MonacoApi,
  monacoDensityOptions,
  monacoLanguage,
} from "./monaco-loader";
import { ScrollStateService } from "./scroll-state.service";
import { attachReviewComments, MonacoReviewApi } from "./review/review-comments.monaco";
import { KjButtonComponent } from "@kouji-ui/components";

/**
 * WRITABLE single-file editor (B1.1) — the Monaco replacement for the
 * read-only `UnifiedCodeComponent` file mode. The EditsStore buffer, not the
 * Monaco model, is the source of truth: keystrokes flow into the store, the
 * editor cap can demote this instance to plain text without losing edits, and
 * `newText` (fresh disk content) only replaces the buffer when it is clean.
 *
 * Review comments (hover +, drag range, composer, cards) work exactly as in
 * the CM surface via `attachReviewComments`. Find widget, multi-cursor,
 * folding, bracket matching, undo — Monaco built-ins, nothing to wire.
 */
@Component({
  selector: "app-monaco-file-editor",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjButtonComponent],
  template: `
    <div #host class="code-host"></div>
    <!-- B4.3: revert-hunk popover, anchored at the marker click. Hand-rolled on
         purpose: kj-popover / kj-confirm-popup only anchor to a trigger element
         (KjOverlayTriggerLike) and Monaco's gutter markers aren't Angular DOM,
         so there is nothing to hand the library — it has no manual-coords API. -->
    @if (revertAsk(); as ra) {
      <div class="popover gm-pop rise" [style.left.px]="ra.x" [style.top.px]="ra.y" (mousedown)="$event.stopPropagation()">
        <span class="gm-pop-label">{{ ra.hunk.newLines === 0 ? 'Restore deleted lines?' : 'Revert this hunk to HEAD?' }}</span>
        <kj-button kjVariant="danger" (click)="confirmRevert(ra.hunk)">Revert</kj-button>
        <kj-button kjVariant="outline" (click)="revertAsk.set(null)">Cancel</kj-button>
      </div>
    }
  `,
  styles: [
    `
      /* fill + .code-host + the Monaco surface recolour are shared recipes in
         styles.css; only the positioning context for .gm-pop is local */
      :host {
        position: relative;
      }
      /* B4.3 change markers in the line-decorations column */
      :host ::ng-deep .gm-added {
        background: var(--sem-add);
        width: 3px !important;
        cursor: pointer;
      }
      :host ::ng-deep .gm-modified {
        background: var(--sem-change);
        width: 3px !important;
        cursor: pointer;
      }
      :host ::ng-deep .gm-deleted {
        background: linear-gradient(to bottom, transparent 60%, var(--sem-del) 60%);
        width: 7px !important;
        cursor: pointer;
      }
      /* surface comes from .popover */
      .gm-pop {
        position: fixed;
        z-index: 70;
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-4);
      }
      .gm-pop-label {
        color: var(--ink-2);
      }
    `,
  ],
})
export class MonacoFileEditorComponent {
  private readonly review = inject(ReviewStore);
  private readonly ui = inject(UiStore);
  private readonly editorNav = inject(EditorNavService);
  private readonly edits = inject(EditsStore);
  private readonly scroll = inject(ScrollStateService);

  // Inputs: decorator @Input backed by signals (vitest JIT / NG0950 pattern).
  readonly agent = signal("");
  readonly file = signal("");
  readonly newText = signal("");
  readonly lang = signal("");
  /** Host bumps this after mutating the EditsStore buffer directly (e.g. the
   *  conflict banner's Reload) so the live model re-syncs from the buffer. */
  readonly syncGen = signal(0);
  /** B4.3: changed regions vs HEAD — rendered as gutter change markers. */
  readonly hunks = signal<FileHunk[]>([]);

  @Input("agent") set agentInput(v: string) { this.agent.set(v); }
  @Input("file") set fileInput(v: string) { this.file.set(v); }
  @Input("newText") set newTextInput(v: string) { this.newText.set(v); }
  @Input("lang") set langInput(v: string) { this.lang.set(v); }
  @Input("syncGen") set syncGenInput(v: number) { this.syncGen.set(v); }
  @Input("hunks") set hunksInput(v: FileHunk[]) { this.hunks.set(v ?? []); }

  /** Marker click confirmed — the host runs the backend revert. */
  @Output() readonly revertHunk = new EventEmitter<FileHunk>();
  /** Pending revert confirmation (screen coords of the marker click). */
  readonly revertAsk = signal<{ hunk: FileHunk; x: number; y: number } | null>(null);

  private readonly host = viewChild.required<ElementRef<HTMLElement>>("host");
  private monaco: MonacoApi | null = null;
  private editor: monacoApi.editor.IStandaloneCodeEditor | null = null;
  private model: monacoApi.editor.ITextModel | null = null;
  private reviewApi: MonacoReviewApi | null = null;
  private unregisterCap: (() => void) | null = null;
  private hunkDecos: monacoApi.editor.IEditorDecorationsCollection | null = null;
  private hunkSub: monacoApi.IDisposable | null = null;
  /** Bumped when a fresh editor is live, so dependent effects re-push. */
  private readonly viewGen = signal(0);
  private renderToken = 0;
  /** Key the live editor was built for — teardown runs after the agent/file
   *  signals already hold the NEXT file, so saving must use this, not them. */
  private mountedKey: { agent: string; file: string } | null = null;
  /** Guards the store-update feedback loop while we setValue programmatically. */
  private applyingExternal = false;

  constructor() {
    // Rebuild the editor when the file identity or language changes. Theme is
    // NOT a rebuild trigger — Monaco themes are global (see theme effect).
    effect(() => {
      void this.render(this.agent(), this.file(), this.lang());
    });
    // Theme switch restyles every live Monaco editor in place.
    // Density switch → push the new code metrics into the live editor. Monaco
    // reads fontSize/lineHeight once at create(), so without this the open file
    // keeps the old density's glyph size until the tab is closed and reopened.
    effect(() => {
      void this.ui.tweaks().density;
      applyMonacoDensity(this.editor);
    });
    effect(() => {
      const theme = this.ui.tweaks().theme;
      if (this.monaco) applyMonacoTheme(this.monaco, theme);
    });
    // Fresh disk content: adopt into the buffer (EditsStore refuses when
    // dirty), then reflect the buffer in the live editor.
    effect(() => {
      const text = this.newText();
      const agent = this.agent();
      const file = this.file();
      this.viewGen();
      this.syncGen();
      if (!agent || !file) return;
      // untracked: this effect must re-run on new disk text / a new view, not
      // on every keystroke's store write (open also writes on first adopt)
      const buf = untracked(() => this.edits.open(agent, file, text));
      const model = this.model;
      if (model && model.getValue() !== buf.text) {
        this.applyingExternal = true;
        try {
          model.setValue(buf.text);
        } finally {
          this.applyingExternal = false;
        }
      }
    });
    // Push comment updates into the live editor (add/remove/clear anywhere).
    effect(() => {
      const agent = this.agent();
      const file = this.file();
      this.viewGen();
      const comments = this.review
        .list(agent)
        .filter((c) => c.file === file && c.view === "file")
        .map((c) => ({ id: c.id, fromLine: c.fromLine, toLine: c.toLine, note: c.note }));
      this.reviewApi?.setComments(comments);
    });
    // B4.3: (re)paint the gutter change markers on the live editor.
    effect(() => {
      const hunks = this.hunks();
      this.viewGen();
      this.renderHunkMarkers(hunks);
    });
    // Go-to-line (B2.3): consume a posted target once this editor is live.
    effect(() => {
      const t = this.editorNav.target();
      this.viewGen();
      const editor = this.editor;
      if (!t || !editor || !this.model) return;
      if (t.agentId !== this.agent() || t.file !== this.file()) return;
      const line = Math.max(1, Math.min(t.line, this.model.getLineCount()));
      const col = Math.max(1, t.col);
      editor.setPosition({ lineNumber: line, column: col });
      editor.revealLineInCenter(line);
      editor.focus();
      this.editorNav.consume(t);
    });
    inject(DestroyRef).onDestroy(() => this.teardown());
  }

  private teardown(): void {
    this.unregisterCap?.();
    this.unregisterCap = null;
    this.reviewApi?.dispose();
    this.reviewApi = null;
    this.hunkSub?.dispose();
    this.hunkSub = null;
    this.hunkDecos = null;
    this.revertAsk.set(null);
    if (this.editor && this.mountedKey) {
      this.scroll.saveView(this.mountedKey.agent, this.mountedKey.file, this.editor.saveViewState());
    }
    this.mountedKey = null;
    this.editor?.dispose();
    this.editor = null;
    this.model?.dispose();
    this.model = null;
  }

  /** B4.3: markers in the line-decorations column, exact per-hunk regions. */
  private renderHunkMarkers(hunks: FileHunk[]): void {
    const editor = this.editor;
    const monaco = this.monaco;
    const model = this.model;
    if (!editor || !monaco || !model) return;
    this.hunkDecos ??= editor.createDecorationsCollection();
    const lines = model.getLineCount();
    const decos: monacoApi.editor.IModelDeltaDecoration[] = [];
    for (const h of hunks) {
      if (h.newLines === 0) {
        const n = Math.min(Math.max(1, h.newStart), lines);
        decos.push({
          range: new monaco.Range(n, 1, n, 1),
          options: { linesDecorationsClassName: "gm-deleted" },
        });
        continue;
      }
      const cls = h.oldLines === 0 ? "gm-added" : "gm-modified";
      const from = Math.min(h.newStart, lines);
      const to = Math.min(h.newStart + h.newLines - 1, lines);
      decos.push({
        range: new monaco.Range(from, 1, to, 1),
        options: { linesDecorationsClassName: cls },
      });
    }
    this.hunkDecos.set(decos);
  }

  /** The hunk whose marker covers `line` (deleted hunks sit on their boundary). */
  private hunkAtLine(line: number): FileHunk | null {
    for (const h of this.hunks()) {
      if (h.newLines === 0) {
        if (Math.max(1, h.newStart) === line) return h;
      } else if (line >= h.newStart && line < h.newStart + h.newLines) {
        return h;
      }
    }
    return null;
  }

  confirmRevert(h: FileHunk): void {
    this.revertAsk.set(null);
    this.revertHunk.emit(h);
  }

  private async render(agent: string, file: string, lang: string): Promise<void> {
    const token = ++this.renderToken;
    const el = this.host().nativeElement;
    this.teardown();
    if (!agent || !file) return;
    el.textContent = "loading…";

    let monaco: MonacoApi;
    let langId: string;
    try {
      monaco = await loadMonaco();
      langId = await monacoLanguage(lang);
    } catch {
      if (token === this.renderToken) el.textContent = this.bufferText();
      return;
    }
    if (token !== this.renderToken) return;
    this.monaco = monaco;
    applyMonacoTheme(monaco, this.ui.tweaks().theme);

    // Yield a macrotask so the triggering frame paints before the synchronous
    // editor build (same rationale as the CM surfaces).
    await new Promise<void>((r) => setTimeout(r, 0));
    if (token !== this.renderToken) return;
    el.textContent = "";

    try {
      const buf = this.edits.open(agent, file, this.newText());
      const model = monaco.editor.createModel(buf.text, langId);
      const editor = monaco.editor.create(el, {
        model,
        readOnly: false,
        glyphMargin: true, // review-comment hover + / anchors
        wordWrap: "on",
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        ...monacoDensityOptions(),
        fixedOverflowWidgets: true,
        renderLineHighlight: "line",
        stickyScroll: { enabled: false },
      });
      this.model = model;
      this.editor = editor;
      this.mountedKey = { agent, file };
      const viewState = this.scroll.getView(agent, file);
      if (viewState) editor.restoreViewState(viewState);
      this.reviewApi = attachReviewComments(monaco, editor, {
        save: (fromLine, toLine, note) => this.saveComment(fromLine, toLine, note),
        remove: (id) => this.review.remove(this.agent(), id),
      });
      // Keystrokes → buffer. The store is what Ctrl+S writes to disk.
      model.onDidChangeContent(() => {
        if (this.applyingExternal) return;
        this.edits.update(this.agent(), this.file(), model.getValue());
      });
      // B4.3: a click on a change marker opens the revert popover.
      this.hunkSub = editor.onMouseDown((e) => {
        if (e.target.type !== monaco.editor.MouseTargetType.GUTTER_LINE_DECORATIONS) return;
        const line = e.target.position?.lineNumber;
        if (!line) return;
        const hunk = this.hunkAtLine(line);
        if (!hunk) return;
        e.event.preventDefault();
        this.revertAsk.set({ hunk, x: e.event.posx, y: e.event.posy });
      });
      // A0.6 editor cap: demote to plain text, buffer survives in the store.
      this.unregisterCap = registerEditor(() => {
        if (this.editor !== editor) return; // superseded — nothing to demote
        if (this.mountedKey) {
          this.scroll.saveView(this.mountedKey.agent, this.mountedKey.file, editor.saveViewState());
        }
        this.mountedKey = null;
        this.reviewApi?.dispose();
        this.reviewApi = null;
        editor.dispose();
        model.dispose();
        this.editor = null;
        this.model = null;
        el.textContent = this.bufferText();
      });
      this.viewGen.update((n) => n + 1);
    } catch (e) {
      console.warn("[monaco-file-editor] editor build failed, showing plain text", e);
      el.textContent = this.bufferText();
    }
  }

  private bufferText(): string {
    return this.edits.get(this.agent(), this.file())?.text ?? this.newText();
  }

  private saveComment(fromLine: number, toLine: number, note: string): void {
    const model = this.model;
    if (!model) return;
    const lines = model.getLineCount();
    const from = Math.max(1, Math.min(fromLine, lines));
    const to = Math.max(from, Math.min(toLine, lines));
    const text: string[] = [];
    for (let n = from; n <= to; n++) text.push(model.getLineContent(n));
    this.review.add(this.agent(), {
      file: this.file(),
      view: "file",
      lang: this.lang(),
      fromLine: from,
      toLine: to,
      side: "file",
      snippet: (text[0] ?? "").trim(),
      lines: text,
      note,
    });
  }
}
