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
import { BlameLine } from "../models";
import { BlameSpec, blameDecorations, blameSpecs, shaAtLine } from "./monaco-blame";
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
import { MODEL_CACHE } from "./monaco-models";
import { modelUri } from "./nav-providers";
import { NavHintKind, NavProvidersService } from "./nav-providers.service";
import { IconComponent } from "../shared/icon.component";
import { KjButtonComponent, KjSpinnerComponent } from "@kouji-ui/components";

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
  imports: [KjButtonComponent, KjSpinnerComponent, IconComponent],
  // .nav-hinting hides Monaco's own "No definition found" box while our chip
  // is up; data-root lets the opener find the root of a virtual doc's tab
  host: { "[class.nav-hinting]": "hint() !== null", "[attr.data-root]": "agent()", "[attr.data-readonly]": "readOnly() ? '' : null" },
  template: `
    <div #host class="code-host"></div>
    <!-- M2/M3/M4 NavHint (design .nav-hint): a quiet bottom-right chip that
         fades on its own — "no definition found", the LSP fallback "jdtls
         starting… showing index result", or why the library index had
         nothing (no JDK / a sources jar not downloaded) -->
    @if (hint(); as h) {
      <span class="nav-hint" data-testid="nav-hint" [attr.data-kind]="h.kind">
        @switch (h.kind) {
          @case ('fallback') {
            <kj-spinner kjSize="xs" kjAriaLabel="language server starting" />{{ h.label || 'language server' }} starting… showing index result
          }
          @case ('jdk-missing') {
            <app-icon name="warn" size="sm" />JDK not found — set JAVA_HOME
          }
          @case ('sources-missing') {
            <app-icon name="warn" size="sm" />sources jar not downloaded — run mvn dependency:sources
          }
          @default {
            <app-icon name="search" size="sm" />no definition found
          }
        }
      </span>
    }
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
      /* Annotate: the blame column is Monaco injected text at column 1, so it
         is styled through a class name (no inline style is possible there).
         Fixed hue buckets stand in for the continuous per-author hue. */
      :host ::ng-deep .blm {
        border-right: 1px solid var(--hair);
        cursor: pointer;
        white-space: pre;
      }
      :host ::ng-deep .blm-cont {
        color: transparent;
      }
      :host ::ng-deep .blm-h0 { color: hsl(15, 60%, 66%); }
      :host ::ng-deep .blm-h1 { color: hsl(45, 60%, 66%); }
      :host ::ng-deep .blm-h2 { color: hsl(75, 60%, 66%); }
      :host ::ng-deep .blm-h3 { color: hsl(105, 60%, 66%); }
      :host ::ng-deep .blm-h4 { color: hsl(135, 60%, 66%); }
      :host ::ng-deep .blm-h5 { color: hsl(165, 60%, 66%); }
      :host ::ng-deep .blm-h6 { color: hsl(195, 60%, 66%); }
      :host ::ng-deep .blm-h7 { color: hsl(225, 60%, 66%); }
      :host ::ng-deep .blm-h8 { color: hsl(255, 60%, 66%); }
      :host ::ng-deep .blm-h9 { color: hsl(285, 60%, 66%); }
      :host ::ng-deep .blm-h10 { color: hsl(315, 60%, 66%); }
      :host ::ng-deep .blm-h11 { color: hsl(345, 60%, 66%); }
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
  private readonly nav = inject(NavProvidersService);

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
  /** Annotate: per-line blame, rendered as an injected-text column. Empty =
   *  Annotate off, and the editor is exactly what it was before. */
  readonly blame = signal<BlameLine[]>([]);
  /** M3 virtual docs: `file` is a virtual uri and `newText` its content —
   *  no EditsStore buffer, no review, no hunks/blame, Monaco read-only. */
  readonly readOnly = signal(false);

  @Input("agent") set agentInput(v: string) { this.agent.set(v); }
  @Input("file") set fileInput(v: string) { this.file.set(v); }
  @Input("newText") set newTextInput(v: string) { this.newText.set(v); }
  @Input("lang") set langInput(v: string) { this.lang.set(v); }
  @Input("syncGen") set syncGenInput(v: number) { this.syncGen.set(v); }
  @Input("hunks") set hunksInput(v: FileHunk[]) { this.hunks.set(v ?? []); }
  @Input("blame") set blameInput(v: BlameLine[]) { this.blame.set(v ?? []); }
  @Input("readOnly") set readOnlyInput(v: boolean) { this.readOnly.set(!!v); }

  /** Marker click confirmed — the host runs the backend revert. */
  @Output() readonly revertHunk = new EventEmitter<FileHunk>();
  /** Blame gutter click — the host opens that commit's diff. */
  @Output() readonly openCommit = new EventEmitter<string>();
  /** Pending revert confirmation (screen coords of the marker click). */
  readonly revertAsk = signal<{ hunk: FileHunk; x: number; y: number } | null>(null);
  /** The NavHint currently shown (null = none). */
  readonly hint = signal<{ kind: NavHintKind; label?: string } | null>(null);
  private hintTimer: ReturnType<typeof setTimeout> | null = null;
  /** The last NavHint event this editor showed (fallback events stay posted). */
  private seenHintN = 0;

  private readonly host = viewChild.required<ElementRef<HTMLElement>>("host");
  private monaco: MonacoApi | null = null;
  private editor: monacoApi.editor.IStandaloneCodeEditor | null = null;
  private model: monacoApi.editor.ITextModel | null = null;
  private reviewApi: MonacoReviewApi | null = null;
  private unregisterCap: (() => void) | null = null;
  private hunkDecos: monacoApi.editor.IEditorDecorationsCollection | null = null;
  private hunkSub: monacoApi.IDisposable | null = null;
  /** Keystrokes → buffer. Disposed on teardown: the model outlives the
   *  editor in the ModelCache, and a stale listener would keep writing. */
  private contentSub: monacoApi.IDisposable | null = null;
  /** The cache key the live model was acquired under. */
  private modelKey: string | null = null;
  private blameDecos: monacoApi.editor.IEditorDecorationsCollection | null = null;
  /** Specs behind the live blame column — the gutter click reads its sha here. */
  private blameLive: BlameSpec[] = [];
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
      this.readOnly();
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
      if (this.readOnly()) {
        // a virtual doc has no buffer: the content IS the model
        const m = this.model;
        if (m && m.getValue() !== text) m.setValue(text);
        return;
      }
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
      if (this.readOnly()) return; // nothing to comment on in a library doc
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
    // Annotate: (re)paint the blame column, and make the buffer read-only while
    // it is on — the column is a view of a past state, not a place to type.
    effect(() => {
      const blame = this.blame();
      this.viewGen();
      this.renderBlame(blame);
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
    // M2: a "no definition" answer for THIS file shows the hint chip; the CSS
    // fade runs 3.4s and the signal clears right after so the next one
    // restarts the animation from a fresh element.
    // M3: the "server starting… showing index result" fallback is about the
    // ROOT, not the file — the jump it rides on usually replaces this leaf's
    // editor with the target's, so every editor of that root shows it (the
    // target included) and the service, not the editor, retires the event.
    effect(() => {
      const h = this.nav.hint();
      if (!h || h.n === this.seenHintN || h.id !== this.agent()) return;
      if (h.kind !== "fallback" && h.path !== this.file()) return;
      untracked(() => {
        this.seenHintN = h.n;
        this.showHint(h.kind, h.label);
        if (h.kind !== "fallback") this.nav.hint.set(null); // consumed — a later editor of this file must not replay it
      });
    });
    inject(DestroyRef).onDestroy(() => {
      if (this.hintTimer) clearTimeout(this.hintTimer);
      this.teardown();
    });
  }

  private showHint(kind: NavHintKind, label?: string): void {
    if (this.hintTimer) clearTimeout(this.hintTimer);
    this.hint.set(null);
    // next tick so an identical hint re-mounts the element (restarts the fade)
    this.hintTimer = setTimeout(() => {
      this.hint.set({ kind, label });
      this.hintTimer = setTimeout(() => {
        this.hint.set(null);
        this.hintTimer = null;
      }, 3400);
    }, 0);
  }

  private teardown(): void {
    this.unregisterCap?.();
    this.unregisterCap = null;
    this.reviewApi?.dispose();
    this.reviewApi = null;
    this.hunkSub?.dispose();
    this.hunkSub = null;
    this.hunkDecos = null;
    this.blameDecos = null;
    this.blameLive = [];
    this.revertAsk.set(null);
    if (this.editor && this.mountedKey) {
      this.scroll.saveView(this.mountedKey.agent, this.mountedKey.file, this.editor.saveViewState());
    }
    this.mountedKey = null;
    this.contentSub?.dispose();
    this.contentSub = null;
    this.editor?.dispose();
    this.editor = null;
    this.releaseModel();
  }

  /** Hand the model back to the ModelCache — never dispose: peek targets and
   *  a fast reopen read it from there. */
  private releaseModel(): void {
    if (this.monaco && this.modelKey) MODEL_CACHE.release(this.monaco, this.modelKey);
    this.modelKey = null;
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

  /** Annotate: the injected blame column, or nothing when Annotate is off. */
  private renderBlame(lines: BlameLine[]): void {
    const editor = this.editor;
    const monaco = this.monaco;
    const model = this.model;
    if (!editor || !monaco || !model) return;
    this.blameDecos ??= editor.createDecorationsCollection();
    this.blameLive = lines.length ? blameSpecs(lines) : [];
    this.blameDecos.set(
      this.blameLive.length
        ? blameDecorations(monaco, this.blameLive, model.getLineCount())
        : [],
    );
    // a virtual doc is read-only regardless of the blame column
    editor.updateOptions({ readOnly: this.readOnly() || this.blameLive.length > 0 });
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
    const readOnly = this.readOnly();
    // M2: navigation providers once per language, the opener once, and the
    // symbol index of this root warmed (idempotent on the backend; a virtual
    // doc belongs to no root's index)
    this.nav.ensureOpener(monaco);
    this.nav.ensureLanguage(monaco, langId);
    if (!readOnly) this.nav.startIndex(agent);

    // Yield a macrotask so the triggering frame paints before the synchronous
    // editor build (same rationale as the CM surfaces).
    await new Promise<void>((r) => setTimeout(r, 0));
    if (token !== this.renderToken) return;
    el.textContent = "";

    try {
      if (readOnly) {
        // M3 virtual doc: the uri IS the model key (peek targets share it),
        // the text is the payload, nothing writes back
        const model = MODEL_CACHE.acquire(monaco, file, this.newText(), langId);
        this.modelKey = file;
        const editor = monaco.editor.create(el, {
          model,
          readOnly: true,
          readOnlyMessage: { value: "This file comes from a library source and cannot be edited." },
          glyphMargin: false,
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
        this.unregisterCap = registerEditor(() => {
          if (this.editor !== editor) return;
          this.mountedKey = null;
          editor.dispose();
          this.editor = null;
          this.releaseModel();
          el.textContent = this.newText();
        });
        this.viewGen.update((n) => n + 1);
        return;
      }
      const buf = this.edits.open(agent, file, this.newText());
      const key = modelUri(agent, file);
      const model = MODEL_CACHE.acquire(monaco, key, buf.text, langId);
      this.modelKey = key;
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
      this.contentSub = model.onDidChangeContent(() => {
        if (this.applyingExternal) return;
        this.edits.update(this.agent(), this.file(), model.getValue());
      });
      // B4.3: a click on a change marker opens the revert popover.
      this.hunkSub = editor.onMouseDown((e) => {
        // Annotate: a click in the blame column opens that commit. The injected
        // text sits INSIDE the content area, so the mouse target is ordinary
        // text — the class on the clicked node is what tells it apart.
        if (this.blameLive.length) {
          const el = e.event.target as HTMLElement | null;
          if (el?.closest?.(".blm")) {
            const line = e.target.position?.lineNumber;
            const sha = line ? shaAtLine(this.blameLive, line) : null;
            e.event.preventDefault();
            if (sha) this.openCommit.emit(sha);
            return;
          }
        }
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
        this.contentSub?.dispose();
        this.contentSub = null;
        editor.dispose();
        this.editor = null;
        this.releaseModel();
        el.textContent = this.bufferText();
      });
      this.viewGen.update((n) => n + 1);
    } catch (e) {
      console.warn("[monaco-file-editor] editor build failed, showing plain text", e);
      el.textContent = this.bufferText();
    }
  }

  private bufferText(): string {
    if (this.readOnly()) return this.newText();
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
