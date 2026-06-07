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
import { MergeView } from "@codemirror/merge";
import { EditorState, Extension } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { UiStore } from "../ui/ui.store";
import { editorTheme, langExt } from "./code-lang";

@Component({
  selector: "app-code-diff",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div
    #host
    style="height:100%;overflow:auto;font-size:12.5px"
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

  constructor() {
    // re-renders on content OR theme change (light/dark need different highlights)
    effect(() => this.render(this.oldText(), this.newText(), this.lang(), this.ui.tweaks().theme));
  }
  ngOnDestroy() {
    this.view?.destroy();
  }

  private render(oldText: string, newText: string, lang: string, theme: "dark" | "light") {
    const el = this.host().nativeElement;
    this.view?.destroy();
    el.innerHTML = "";
    const exts: Extension[] = [
      lineNumbers(),
      editorTheme(theme),
      EditorView.editable.of(false),
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      langExt(lang),
    ];
    this.view = new MergeView({
      a: { doc: oldText, extensions: exts },
      b: { doc: newText, extensions: exts },
      parent: el,
      collapseUnchanged: { margin: 3, minSize: 4 },
      gutter: true,
      highlightChanges: true, // mark the changed run within a line (.cm-changedText)
    });
  }
}
