import { ChangeDetectionStrategy, Component, effect, ElementRef, input, OnDestroy, viewChild } from "@angular/core";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { MergeView } from "@codemirror/merge";
import { EditorState, Extension } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView, lineNumbers } from "@codemirror/view";

function langExt(lang: string): Extension {
  switch (lang) {
    case "javascript": return javascript({ typescript: true });
    case "json": return json();
    case "css": return css();
    case "html": return html();
    case "markdown": return markdown();
    case "rust": return rust();
    case "python": return python();
    default: return [];
  }
}

@Component({
  selector: "app-code-diff",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div #host style="height:100%;overflow:auto;font-size:12.5px"></div>`,
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
    });
  }
}
