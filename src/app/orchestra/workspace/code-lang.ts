import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { java } from "@codemirror/lang-java";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Extension } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView } from "@codemirror/view";

/**
 * Syntax-highlight theme matched to the app theme. oneDark for dark mode; for
 * light mode a light highlight (CodeMirror's default dark-on-light token colors +
 * readable base text), since oneDark's light-on-dark palette — especially its
 * yellows — washes out on a light background.
 */
export function editorTheme(appTheme: "dark" | "light"): Extension {
  if (appTheme === "light") {
    return [
      EditorView.theme({
        "&": { color: "var(--ink-2)" },
        ".cm-gutters": { color: "var(--ink-4)" },
      }),
      syntaxHighlighting(defaultHighlightStyle),
    ];
  }
  return oneDark;
}

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
