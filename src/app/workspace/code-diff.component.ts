import {
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  inject,
  input,
  OnDestroy,
  signal,
  viewChild,
} from "@angular/core";
import type { Extension } from "@codemirror/state";
import { UiStore } from "../ui/ui.store";
import { buildTheme, CMCore, loadCMCore, loadLangExt } from "./code-lang";

/** Count '\n' without allocating a split array (cheap pre-check on big strings). */
function countNewlines(s: string): number {
  let n = 0;
  for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) === 10) n++;
  return n;
}

/**
 * The MergeView char-level diff explodes on MANY LONG lines (generated JSON/CSV/
 * SVG, minified or wholesale-reformatted files) — measured at 0.7s for 1000×2k-char
 * lines, ~5s at 3000 lines, ~16s at 8k-char lines, all on the main thread. File
 * size alone is NOT the signal: 20k short lines or a 1MB single line both build in
 * <35ms. The discriminator is a high AVERAGE line length on a large file. A plain
 * (non-diff) editor renders any of these in <25ms, so we fall back to it.
 */
export function diffWouldStall(oldText: string, newText: string): boolean {
  const total = oldText.length + newText.length;
  if (total < 200_000) return false; // small files always diff fast
  const lineCount = countNewlines(oldText) + countNewlines(newText) + 2;
  return total / lineCount > 1000; // long average line → pathological
}

@Component({
  selector: "app-code-diff",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (tooLarge()) {
      <div class="diff-toobig">
        <span>Large file with long lines — showing content without inline diff (a full diff would freeze the editor).</span>
        <button class="db-btn" (click)="forceDiff.set(true)">Diff anyway</button>
      </div>
    }
    <div #host class="diff-host"></div>
  `,
  styles: [
    `
      /* fill the diff body so the editor takes ALL available height */
      :host {
        display: flex;
        flex-direction: column;
        height: 100%;
        min-height: 0;
      }
      .diff-host {
        flex: 1;
        min-height: 0;
        overflow: auto;
        font-size: var(--fs-ui);
      }
      /* fallback notice band shown when a pathological diff is rendered plain */
      .diff-toobig {
        display: flex;
        align-items: center;
        gap: var(--sp-4);
        padding: var(--sp-2) var(--sp-5);
        flex: none;
        background: var(--panel);
        border-bottom: 1px solid var(--hair);
        font-size: var(--fs-xs);
        color: var(--ink-3);
      }
      .db-btn {
        margin-left: auto;
        flex: none;
        padding: var(--sp-1) var(--sp-4);
        border: 1px solid var(--hair-2);
        border-radius: var(--r-sm);
        background: transparent;
        color: var(--ink-2);
        cursor: pointer;
        font-size: var(--fs-xs);
      }
      .db-btn:hover {
        background: var(--panel-2);
        color: var(--ink);
      }
      /* the merge view + its two editors must stretch to the host height */
      :host ::ng-deep .cm-mergeView,
      :host ::ng-deep .cm-mergeViewEditors {
        height: 100%;
      }
      :host ::ng-deep .cm-editor {
        height: 100% !important;
      }
      :host ::ng-deep .cm-scroller {
        height: 100%;
      }
      /* fill the panel even when the document is short: the content + gutter
         track the scroller height so the line-number column + editor bg run
         all the way to the bottom (no empty un-guttered area below the last line) */
      :host ::ng-deep .cm-content {
        min-height: 100% !important;
      }
      :host ::ng-deep .cm-gutters {
        min-height: 100% !important;
      }

      /* ----- single flat change layer -----
         The row tint lives ONLY on .cm-changedLine (the line/row decoration).
         The inline ins/del marks (.cm-insertedLine / .cm-deletedLine) and the
         within-line .cm-changedText mark must stay transparent so there is no
         inner box/overlay/underline. merge-b side = additions (green),
         merge-a side / deleted chunk = deletions (red). */
      :host ::ng-deep .cm-merge-b .cm-changedLine { background-color: rgba(52, 224, 161, 0.15) !important; }
      :host ::ng-deep .cm-merge-a .cm-changedLine,
      :host ::ng-deep .cm-deletedChunk { background-color: rgba(255, 93, 122, 0.15) !important; }
      /* full-line ins/del markers stay flat — the row tint on .cm-changedLine carries it */
      :host ::ng-deep .cm-insertedLine,
      :host ::ng-deep .cm-deletedLine,
      :host ::ng-deep .cm-deletedLine del {
        background: none !important;
        background-color: transparent !important;
        text-decoration: none !important;
      }
      /* within-line changed text: brightly highlight only the changed sub-word
         (word-level diff — e.g. "Humain" in greet → greetHumain) over the row tint */
      :host ::ng-deep .cm-merge-b .cm-changedText {
        background-color: rgba(52, 224, 161, 0.34) !important;
        border-radius: 2px;
      }
      :host ::ng-deep .cm-merge-a .cm-changedText,
      :host ::ng-deep .cm-deletedChunk .cm-deletedText {
        background-color: rgba(255, 93, 122, 0.34) !important;
        border-radius: 2px;
        text-decoration: none !important;
      }
      /* keep oneDark for syntax colors, but force OUR background (not its #282c34) */
      :host ::ng-deep .cm-editor {
        background-color: var(--bg) !important;
      }
      :host ::ng-deep .cm-gutters {
        background-color: var(--bg) !important;
      }
    `,
  ],
})
export class CodeDiffComponent implements OnDestroy {
  readonly oldText = input<string>("");
  readonly newText = input<string>("");
  readonly lang = input<string>("");

  private ui = inject(UiStore);
  private host = viewChild.required<ElementRef<HTMLElement>>("host");
  private view?: { destroy(): void };
  // bumped on every render so a late-arriving lazy parser chunk is ignored if the
  // view has since been rebuilt (content / theme / lang changed underneath it).
  private renderToken = 0;

  /** True when the current file was rendered plain (diff would stall). */
  readonly tooLarge = signal(false);
  /** User clicked "Diff anyway" — bypass the stall guard for this file. */
  readonly forceDiff = signal(false);

  constructor() {
    // re-renders on content OR theme change (light/dark need different highlights),
    // and when the user forces a full diff on a flagged-large file.
    effect(() =>
      this.render(this.oldText(), this.newText(), this.lang(), this.ui.tweaks().theme, this.forceDiff()),
    );
    // Reset the override whenever the file content changes so each new file starts
    // guarded again. Does NOT read forceDiff, so this can't loop with the render effect.
    effect(() => {
      this.oldText();
      this.newText();
      this.forceDiff.set(false);
    });
  }
  ngOnDestroy() {
    this.view?.destroy();
  }

  private async render(
    oldText: string,
    newText: string,
    lang: string,
    theme: "dark" | "light",
    forceDiff: boolean,
  ) {
    const token = ++this.renderToken;
    const el = this.host().nativeElement;
    this.view?.destroy();
    this.view = undefined;
    el.innerHTML = "";

    // core editor is fetched from esm.sh (cached after first diff). Until it lands,
    // show a hint; if it can't load (offline / CDN down), fall back to plain text.
    el.textContent = "loading…";
    let cm: CMCore;
    try {
      cm = await loadCMCore();
    } catch {
      if (token === this.renderToken) el.textContent = newText;
      return;
    }
    if (token !== this.renderToken) return; // a newer render superseded this one

    // A pathological diff (many long lines) makes the MergeView build block the
    // main thread for seconds (measured up to 16s) — disabling highlightChanges
    // or wrapping does NOT help, and the built-in diffConfig bound doesn't either.
    // So fall back to a plain editor (always <25ms), unless the user forced it.
    const plain = !forceDiff && diffWouldStall(oldText, newText);
    this.tooLarge.set(plain);

    // Building the editor is SYNCHRONOUS. Until here we've only awaited microtasks
    // (the cached core load), so without yielding the build would run in the same
    // frame as the change detection that triggered this render — no paint, no
    // input. Hand the browser a macrotask first so the triggering frame paints and
    // the app stays responsive (and the "showing without diff" banner appears).
    await new Promise<void>((r) => setTimeout(r, 0));
    if (token !== this.renderToken) return;
    el.textContent = "";

    try {
      const { EditorState, Compartment } = cm.state;
      const { EditorView, lineNumbers } = cm.view;
      const { MergeView } = cm.merge;
      // language parser is injected lazily via this compartment once it loads
      const langComp = new Compartment();
      const exts: Extension[] = [
        lineNumbers(),
        buildTheme(cm, theme),
        EditorView.editable.of(false),
        EditorState.readOnly.of(true),
        EditorView.lineWrapping,
        langComp.of([]),
      ];
      if (plain) {
        // Non-diff fallback: render the NEW content only. No merge diff = no stall.
        const ev = new EditorView({ doc: newText, extensions: exts, parent: el });
        this.view = ev;
        if (lang) {
          void loadLangExt(lang).then((ext) => {
            if (token !== this.renderToken || this.view !== ev) return; // stale
            ev.dispatch({ effects: langComp.reconfigure(ext) });
          });
        }
      } else {
        const mv = new MergeView({
          a: { doc: oldText, extensions: exts },
          b: { doc: newText, extensions: exts },
          parent: el,
          collapseUnchanged: { margin: 3, minSize: 4 },
          gutter: true,
          highlightChanges: true, // mark the changed run within a line (.cm-changedText)
        });
        this.view = mv;
        if (lang) {
          void loadLangExt(lang).then((ext) => {
            if (token !== this.renderToken || this.view !== mv) return; // stale
            mv.a.dispatch({ effects: langComp.reconfigure(ext) });
            mv.b.dispatch({ effects: langComp.reconfigure(ext) });
          });
        }
      }
    } catch (e) {
      console.warn("[code-diff] editor build failed, showing plain text", e);
      el.textContent = newText;
    }
  }
}
