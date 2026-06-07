import {
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  input,
  OnDestroy,
  viewChild,
} from "@angular/core";
import { EditorState, Extension } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView, lineNumbers } from "@codemirror/view";
import { langExt } from "./code-lang";

/**
 * Read-only, syntax-highlighted view of a single document — used by the file
 * viewer. Same CodeMirror highlighting as the diff (oneDark colors over OUR
 * background), line numbers, soft-wrap, and CM's own vertical scroll.
 */
@Component({
  selector: "app-code-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div #host style="height:100%;overflow:hidden;font-size:12.5px"></div>`,
  styles: [
    `
      :host {
        display: block;
        height: 100%;
        min-height: 0;
      }
      :host ::ng-deep .cm-editor {
        height: 100% !important;
        background-color: var(--bg) !important;
      }
      /* CM's scroller owns the vertical scroll when content overflows */
      :host ::ng-deep .cm-scroller {
        overflow: auto;
      }
      :host ::ng-deep .cm-gutters {
        background-color: var(--bg) !important;
      }
      :host ::ng-deep .cm-content {
        min-height: 100%;
      }
    `,
  ],
})
export class CodeViewComponent implements OnDestroy {
  readonly text = input<string>("");
  /** A `FileDiff.lang` tag (e.g. "javascript", "json", "markdown"). */
  readonly lang = input<string>("");

  private host = viewChild.required<ElementRef<HTMLElement>>("host");
  private view?: EditorView;

  constructor() {
    effect(() => this.render(this.text(), this.lang()));
  }
  ngOnDestroy() {
    this.view?.destroy();
  }

  private render(text: string, lang: string) {
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
    this.view = new EditorView({ doc: text, extensions: exts, parent: el });
  }
}
