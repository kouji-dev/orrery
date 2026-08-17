import {
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  inject,
  input,
  OnDestroy,
  viewChild,
} from "@angular/core";
import type * as monacoApi from "monaco-editor";

import { BlameLine } from "../models";
import { UiStore } from "../ui/ui.store";
import { registerEditor } from "./editor-cap";
import { applyMonacoTheme, loadMonaco, MonacoApi, monacoLanguage } from "./monaco-loader";

/** Stable per-author hue (mirrors AuthorAvatarComponent / file-blame). */
function authorColor(author: string): string {
  let hash = 0;
  for (let i = 0; i < author.length; i++) hash = ((hash * 31) + author.charCodeAt(i)) >>> 0;
  const hue = ((hash % 300) + 30) % 360;
  return `hsl(${hue}, 60%, 66%)`;
}

// ----- dynamic style rules for blame injected-text (color + age fade) -----

const BLAME_STYLE_ID = "cd-blame-styles";
const blameClasses = new Set<string>();

function blameSheet(): HTMLStyleElement {
  let el = document.getElementById(BLAME_STYLE_ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = BLAME_STYLE_ID;
    document.head.appendChild(el);
  }
  return el;
}

/** Class for one (author, fade-bucket) pair; the rule is created on first use.
 *  Only color/background styling — column width comes from the padded content,
 *  so the density token scanner has nothing to flag. */
function blameClass(author: string, fade: number): string {
  let hash = 0;
  const key = `${author}|${fade}`;
  for (let i = 0; i < key.length; i++) hash = ((hash * 33) + key.charCodeAt(i)) >>> 0;
  const cls = `cd-blame-${hash.toString(36)}`;
  if (!blameClasses.has(cls)) {
    blameClasses.add(cls);
    blameSheet().sheet?.insertRule(
      `.${cls} { color: ${authorColor(author)}; background: color-mix(in oklch, var(--ui-ink), transparent ${fade}%); }`,
    );
  }
  return cls;
}

/** Injected-text blame decorations for one side: a fixed-width author column
 *  rendered before each line, IntelliJ-style age fade (recent = stronger). */
export function blameDecorations(
  monaco: MonacoApi,
  lineCount: number,
  lines: BlameLine[],
): monacoApi.editor.IModelDeltaDecoration[] {
  let minW = Infinity;
  let maxW = -Infinity;
  for (const l of lines) {
    if (l.when) {
      if (l.when < minW) minW = l.when;
      if (l.when > maxW) maxW = l.when;
    }
  }
  const span = maxW - minW || 1;
  const out: monacoApi.editor.IModelDeltaDecoration[] = [];
  const n = Math.min(lineCount, lines.length);
  for (let i = 1; i <= n; i++) {
    const b = lines[i - 1];
    if (!b) continue;
    const who = (b.author || "").trim();
    const label = (who.length > 13 ? who.slice(0, 12) + "…" : who).padEnd(14, " ");
    // newest = age 0 (strong tint), oldest = age 1 (faded); uncommitted
    // (when 0) counts as newest so working-tree edits stand out brightest.
    const age = !b.when ? 0 : maxW > -Infinity ? (maxW - b.when) / span : 0;
    const fade = 78 + Math.round(age * 18); // 78%..96% transparent
    out.push({
      range: new monaco.Range(i, 1, i, 1),
      options: {
        before: { content: label, inlineClassName: blameClass(who, fade) },
      },
    });
  }
  return out;
}

/**
 * Side-by-side diff surface (commit / range / history inspection) — Monaco
 * DiffEditor edition (B1.1 migration; formerly CodeMirror `MergeView`). The
 * old/new split is resizable via Monaco's built-in sash, and the diff itself
 * computes in the editor worker (the CM main-thread stall guard is obsolete).
 * The "Annotate" toggle injects a per-line committer column on each side.
 */
@Component({
  selector: "app-code-diff",
  changeDetection: ChangeDetectionStrategy.OnPush,
  standalone: true,
  template: `<div #host class="diff-host"></div>`,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        height: 100%;
        min-height: 0;
      }
      .diff-host {
        flex: 1;
        min-height: 0;
        overflow: hidden;
        font-size: var(--fs-ui);
      }
      :host ::ng-deep .monaco-editor,
      :host ::ng-deep .monaco-editor .margin,
      :host ::ng-deep .monaco-editor-background,
      :host ::ng-deep .monaco-diff-editor {
        background-color: var(--bg) !important;
      }
      /* flat change layer matching the app's diff palette */
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
export class CodeDiffComponent implements OnDestroy {
  readonly oldText = input<string>("");
  readonly newText = input<string>("");
  readonly lang = input<string>("");
  /** Annotate: show a per-line committer column on each side of the diff. */
  readonly showBlame = input<boolean>(false);
  /** Blame for the OLD (left) side — one entry per line of `oldText`. */
  readonly oldBlame = input<BlameLine[]>([]);
  /** Blame for the NEW (right) side — one entry per line of `newText`. */
  readonly newBlame = input<BlameLine[]>([]);

  private ui = inject(UiStore);
  private host = viewChild.required<ElementRef<HTMLElement>>("host");
  private monaco: MonacoApi | null = null;
  private diff: monacoApi.editor.IStandaloneDiffEditor | null = null;
  private models: monacoApi.editor.ITextModel[] = [];
  private blameCollections: monacoApi.editor.IEditorDecorationsCollection[] = [];
  /** Unhooks this component's entry in the global editor cap (A0.6). */
  private unregisterCap: (() => void) | null = null;
  private renderToken = 0;

  constructor() {
    effect(() => {
      void this.render(this.oldText(), this.newText(), this.lang());
    });
    // Theme switch restyles every live Monaco editor in place.
    effect(() => {
      const theme = this.ui.tweaks().theme;
      if (this.monaco) applyMonacoTheme(this.monaco, theme);
    });
    // Blame toggles without rebuilding the editor.
    effect(() => {
      const show = this.showBlame();
      const oldB = this.oldBlame();
      const newB = this.newBlame();
      this.applyBlame(show, oldB, newB);
    });
  }

  ngOnDestroy(): void {
    this.teardown();
  }

  private teardown(): void {
    this.unregisterCap?.();
    this.unregisterCap = null;
    for (const c of this.blameCollections) c.clear();
    this.blameCollections = [];
    this.diff?.dispose();
    this.diff = null;
    for (const m of this.models) m.dispose();
    this.models = [];
  }

  private applyBlame(show: boolean, oldB: BlameLine[], newB: BlameLine[]): void {
    const diff = this.diff;
    const monaco = this.monaco;
    for (const c of this.blameCollections) c.clear();
    this.blameCollections = [];
    if (!show || !diff || !monaco) return;
    const [original, modified] = this.models;
    if (oldB.length && original) {
      this.blameCollections.push(
        diff.getOriginalEditor().createDecorationsCollection(
          blameDecorations(monaco, original.getLineCount(), oldB),
        ),
      );
    }
    if (newB.length && modified) {
      this.blameCollections.push(
        diff.getModifiedEditor().createDecorationsCollection(
          blameDecorations(monaco, modified.getLineCount(), newB),
        ),
      );
    }
  }

  private async render(oldText: string, newText: string, lang: string): Promise<void> {
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
    // editor build.
    await new Promise<void>((r) => setTimeout(r, 0));
    if (token !== this.renderToken) return;
    el.textContent = "";

    try {
      const diff = monaco.editor.createDiffEditor(el, {
        readOnly: true,
        renderSideBySide: true,
        hideUnchangedRegions: { enabled: true, contextLineCount: 3, minimumLineCount: 4 },
        wordWrap: "off", // long lines scroll horizontally within each side
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        fontSize: parseFloat(getComputedStyle(el).fontSize) || 12,
        fixedOverflowWidgets: true,
        renderLineHighlight: "none",
        stickyScroll: { enabled: false },
        renderOverviewRuler: false,
      });
      const original = monaco.editor.createModel(oldText, langId);
      const modified = monaco.editor.createModel(newText, langId);
      diff.setModel({ original, modified });
      this.models = [original, modified];
      this.diff = diff;
      this.applyBlame(this.showBlame(), this.oldBlame(), this.newBlame());
      // A0.6 editor cap: demote to plain text to bound the webview heap.
      this.unregisterCap = registerEditor(() => {
        if (this.diff !== diff) return; // superseded — nothing to demote
        this.teardown();
        el.textContent = newText;
      });
    } catch (e) {
      console.warn("[code-diff] editor build failed, showing plain text", e);
      el.textContent = newText;
    }
  }
}
