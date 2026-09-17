/**
 * Syntax highlighting for the markdown preview's code fences.
 *
 * marked (see marked-mermaid.ts) renders a fence as
 * `.md-box.fence > pre > code.language-X` holding the escaped source, plain.
 * This step colours it with MONACO — already bundled for the editor surfaces
 * and already themed to the app's tokens — so nothing new is added to the
 * dependency tree and a fence reads with the same palette as the same file
 * opened in the editor.
 *
 * Same hydration shape as md-mermaid.ts, and for the same reason: the marked
 * output lands through Angular's [innerHTML] sanitizer, which keeps `class`
 * but drops `data-*` and `id`. So the colouring happens on the LIVE DOM after
 * the binding, and the source + the theme it was coloured for ride on
 * `el.dataset` (set from JS, never through the sanitizer) — a theme toggle
 * then recolours in place from the stored source rather than reading back the
 * span soup, and re-running on the same theme is a no-op.
 *
 * `el.innerHTML = …` on the colorize output bypasses the sanitizer by design:
 * `monaco.editor.colorize` emits nothing but `<span class="mtkN">` + `<br/>`.
 * Its `.mtkN` colour rules are GLOBAL (the `monaco-colors` stylesheet, which
 * colorize() installs itself via registerEditorContainer) — NOT scoped under
 * `.monaco-editor` — so the fence needs no wrapper class, and keeps the app's
 * own font/size/padding from the `.md-box.fence pre` recipe in styles.css.
 *
 * Monaco (a lazy chunk) loads only when the document actually holds a fence in
 * a language it can tokenize: a plain-prose doc, or one whose fences are all
 * `text`/`diff`/unknown, loads nothing. A failure never breaks the document —
 * every block is its own try/catch and simply stays plain.
 */

import { langId } from "../../utils";
import { applyMonacoTheme, loadMonaco, monacoLanguage } from "../monaco-loader";

/** Info-string words that mean "this is not source": leave the fence plain
 *  rather than tokenizing prose, a diff, or a captured stream. */
const NO_LANG = new Set(["text", "plain", "plaintext", "txt", "none", "diff", "patch", "raw", "output"]);

/**
 * Info-string word → the canonical `langId()` tag. langId() speaks EXTENSIONS
 * (it maps file names), so it already answers `ts`, `rs`, `py`, `kt`, `rb`,
 * `sh`, `yml`… — this map only covers what people actually type in a fence and
 * an extension lookup would miss: the full language names, and a handful of
 * spellings with no extension of their own.
 */
const ALIAS: Record<string, string> = {
  typescript: "javascript", javascript: "javascript", node: "javascript", ecmascript: "javascript",
  jsonc: "json", json5: "json",
  rust: "rust", python: "python", python3: "python",
  bash: "shell", sh: "shell", shell: "shell", zsh: "shell", console: "shell", "shell-session": "shell",
  yaml: "yaml", go: "go", golang: "go", sql: "sql", html: "html", css: "css",
  scss: "sass", sass: "sass", less: "less",
  java: "java", kotlin: "kotlin", ruby: "ruby", php: "php",
  c: "cpp", cpp: "cpp", "c++": "cpp", cxx: "cpp", objc: "objectivec", "objective-c": "objectivec",
  csharp: "csharp", "c#": "csharp", cs: "csharp",
  swift: "swift", dockerfile: "dockerfile", docker: "dockerfile",
  powershell: "powershell", pwsh: "powershell", ps1: "powershell",
  toml: "toml", ini: "properties", properties: "properties", conf: "properties", env: "properties",
  xml: "xml", svg: "xml", lua: "lua", r: "r", dart: "dart", scala: "scala",
  clojure: "clojure", clj: "clojure", julia: "julia", perl: "perl",
  markdown: "markdown", md: "markdown", vue: "vue", angular: "angular", groovy: "groovy",
};

/**
 * A fence's info word → the canonical grammar tag, or "" for "leave it plain".
 * Explicit non-languages first, then the typed-out names, then langId() for
 * everything that happens to be spelled like an extension.
 */
export function fenceTag(word: string): string {
  const w = word.trim().toLowerCase();
  if (!w || NO_LANG.has(w)) return "";
  return ALIAS[w] ?? langId("x." + w);
}

interface Job {
  el: HTMLElement;
  src: string;
  tag: string;
}

/**
 * Colour every highlightable fence inside `host` for the given theme: fences
 * not yet coloured, plus already-coloured ones whose stored theme no longer
 * matches (theme toggle). Resolves once every block has been attempted; a
 * block that cannot be coloured is left exactly as marked rendered it.
 */
export async function renderCodeBlocks(host: HTMLElement, theme: "dark" | "light"): Promise<void> {
  const jobs: Job[] = [];
  const nodes = host.querySelectorAll<HTMLElement>('.md-box.fence pre code[class*="language-"]');
  for (const el of Array.from(nodes)) {
    if (el.dataset["mdcTheme"] === theme) continue; // already this theme's colours
    const tag = fenceTag(/(?:^|\s)language-(\S+)/.exec(el.className)?.[1] ?? "");
    if (!tag) continue;
    jobs.push({ el, src: el.dataset["mdcSrc"] ?? el.textContent ?? "", tag });
  }
  // Nothing to colour → Monaco must not load at all.
  if (jobs.length === 0) return;

  let monaco;
  try {
    monaco = await loadMonaco();
  } catch {
    return; // chunk failed: every fence stays plain, retried on the next pass
  }
  // Theme state is global in Monaco — one call is what makes colorize() emit
  // the app's palette, and what recolours the fences on a toggle.
  try {
    applyMonacoTheme(monaco, theme);
  } catch {
    /* tokens unreadable: Monaco's own base theme stands */
  }

  for (const { el, src, tag } of jobs) {
    if (!el.isConnected) continue;
    try {
      const id = await monacoLanguage(tag);
      if (id === "plaintext") continue; // no Monarch grammar — nothing to gain
      const html = await monaco.editor.colorize(src, id, { tabSize: 2 });
      if (!el.isConnected) continue;
      // colorize() closes every line with <br/>, including the last one, which
      // inside <pre> would show as a trailing blank line
      el.innerHTML = html.replace(/<br\/>\s*$/, "");
      el.dataset["mdcSrc"] = src;
      el.dataset["mdcTheme"] = theme;
    } catch {
      /* this one fence stays plain — the document is unaffected */
    }
  }
}
