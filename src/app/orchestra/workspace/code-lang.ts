import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { java } from "@codemirror/lang-java";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";
import { Extension } from "@codemirror/state";

/** Map a `FileDiff.lang` tag to its CodeMirror language extension (empty for unknown). */
export function langExt(lang: string): Extension {
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
