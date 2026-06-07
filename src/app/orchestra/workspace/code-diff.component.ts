import {
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  input,
  OnDestroy,
  viewChild,
} from "@angular/core";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { java } from "@codemirror/lang-java";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";
import { MergeView } from "@codemirror/merge";
import { EditorState, Extension } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView, lineNumbers } from "@codemirror/view";

function langExt(lang: string): Extension {
  switch (lang) {
    case "javascript":
      return javascript({ typescript: true });
    case "json":
      return json();
    case "css":
      return css();
    case "html":
      return html();
    case "markdown":
      return markdown();
    case "rust":
      return rust();
    case "python":
      return python();
    case "java":
      return java();
    case "yaml":
      return yaml();
    default:
      return [];
  }
}

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
      :host ::ng-deep .cm-changedText,
      :host ::ng-deep .cm-insertedLine,
      :host ::ng-deep .cm-deletedLine,
      :host ::ng-deep .cm-deletedLine del,
      :host ::ng-deep .cm-deletedText {
        background: none !important;
        background-color: transparent !important;
        text-decoration: none !important;
      }
    `,
  ],
})
export class CodeDiffComponent implements OnDestroy {
  readonly oldText = input<string>("");
  readonly newText = input<string>("");
  readonly lang = input<string>("");

  private host = viewChild.required<ElementRef<HTMLElement>>("host");
  private view?: MergeView;

  constructor() {
    effect(() => this.render(this.oldText(), this.newText(), this.lang()));
  }
  ngOnDestroy() {
    this.view?.destroy();
  }

  private render(oldText: string, newText: string, lang: string) {
    const el = this.host().nativeElement;
    this.view?.destroy();
    el.innerHTML = "";
    const exts: Extension[] = [
      lineNumbers(),
      oneDark,
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
      highlightChanges: false,
    });
  }
}
