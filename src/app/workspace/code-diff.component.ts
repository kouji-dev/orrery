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
import type { MergeView } from "@codemirror/merge";
import type { Extension } from "@codemirror/state";
import { UiStore } from "../ui/ui.store";
import { buildTheme, CMCore, loadCMCore, loadLangExt } from "./code-lang";

@Component({
  selector: "app-code-diff",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div
    #host
    style="height:100%;overflow:auto;font-size:var(--fs-ui)"
  ></div>`,
  styles: [
    `
      /* fill the diff body so the editor takes ALL available height */
      :host {
        display: block;
        height: 100%;
        min-height: 0;
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
  private view?: MergeView;
  // bumped on every render so a late-arriving lazy parser chunk is ignored if the
  // view has since been rebuilt (content / theme / lang changed underneath it).
  private renderToken = 0;

  constructor() {
    // re-renders on content OR theme change (light/dark need different highlights)
    effect(() => this.render(this.oldText(), this.newText(), this.lang(), this.ui.tweaks().theme));
  }
  ngOnDestroy() {
    this.view?.destroy();
  }

  private async render(oldText: string, newText: string, lang: string, theme: "dark" | "light") {
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

    // Building the MergeView is SYNCHRONOUS and scales with document size — the
    // diff algorithm + DOM construction can block the main thread for hundreds
    // of ms on a large file. Until here we've only awaited microtasks (the
    // cached core load), so the build would otherwise run in the same frame as
    // the change detection that triggered this render: no paint, no input = a
    // hard UI freeze. ALWAYS hand the browser a macrotask first so the build
    // runs in its own task — the triggering frame paints and stays responsive.
    // This matters most for hosts that render <app-code-diff> inline (e.g. a
    // file opened from a commit in the right panel), which — unlike the main
    // diff view — aren't wrapped in @defer to provide that boundary.
    const heavy = oldText.length + newText.length > 80_000;
    if (heavy) el.textContent = "rendering diff…";
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
      const mv = new MergeView({
        a: { doc: oldText, extensions: exts },
        b: { doc: newText, extensions: exts },
        parent: el,
        collapseUnchanged: { margin: 3, minSize: 4 },
        gutter: true,
        // Word-level intra-line highlighting (.cm-changedText) is the costliest
        // MergeView pass; skip it on heavy diffs to keep the build from janking.
        highlightChanges: !heavy,
      });
      this.view = mv;
      if (lang) {
        void loadLangExt(lang).then((ext) => {
          if (token !== this.renderToken || this.view !== mv) return; // stale
          mv.a.dispatch({ effects: langComp.reconfigure(ext) });
          mv.b.dispatch({ effects: langComp.reconfigure(ext) });
        });
      }
    } catch (e) {
      console.warn("[code-diff] editor build failed, showing plain text", e);
      el.textContent = newText;
    }
  }
}
