import type * as monacoApi from "monaco-editor";

/**
 * Lazy loader for Monaco (B1.1 editor migration). Unlike the CodeMirror setup
 * in `code-lang.ts` (esm.sh at runtime), Monaco is BUNDLED: `edcore.main.js`
 * — the editor plus every editor contribution (find widget, folding, bracket
 * matching, multi-cursor, diff editor) but NONE of the language services — is
 * pulled in through a dynamic import, so it lands in its own lazy chunk that
 * loads the first time an editor mounts. Highlighting is Monarch-only via the
 * per-language `basic-languages` contributions, themselves lazy chunks.
 */
export type MonacoApi = typeof monacoApi;

let corePromise: Promise<MonacoApi> | null = null;
let cssAttached = false;

/** Attach (once) the complete Monaco stylesheet — built as the non-injected
 *  "monaco" style bundle in angular.json, so it loads with the first editor
 *  rather than with the app shell. */
function attachMonacoCss(): void {
  if (cssAttached) return;
  cssAttached = true;
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = "monaco.css";
  document.head.appendChild(link);
}

/** Load (once) the Monaco editor core; retries are possible after a failure. */
export function loadMonaco(): Promise<MonacoApi> {
  if (!corePromise) {
    attachMonacoCss();
    // Must be in place before the first editor module executes.
    (self as unknown as { MonacoEnvironment?: unknown }).MonacoEnvironment = {
      getWorker: () =>
        new Worker(new URL("./monaco.worker", import.meta.url), { type: "module" }),
    };
    corePromise = import(
      "monaco-editor/esm/vs/editor/edcore.main.js"
    ) as unknown as Promise<MonacoApi>;
    corePromise.catch(() => (corePromise = null));
  }
  return corePromise;
}

// ---------------------------------------------------------------- themes ----

/** Resolve a CSS design token to a Monaco-safe #rrggbb(aa) hex, or undefined. */
function tokenHex(styles: CSSStyleDeclaration, name: string): string | undefined {
  const raw = styles.getPropertyValue(name).trim();
  if (!raw) return undefined;
  if (raw.startsWith("#")) return raw;
  const m = raw.match(/^rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)(?:[,\s/]+([\d.]+))?\s*\)$/);
  if (!m) return undefined;
  const hex = (n: number): string => Math.round(n).toString(16).padStart(2, "0");
  const alpha = m[4] === undefined ? "" : hex(Number(m[4]) * 255);
  return `#${hex(+m[1])}${hex(+m[2])}${hex(+m[3])}${alpha}`;
}

/**
 * (Re)define + activate the app-matched Monaco theme. Monaco needs concrete
 * hex values, so the app's CSS tokens are resolved from the `[data-theme]`
 * root at call time — call again whenever `ui.tweaks().theme` changes (theme
 * state is global in Monaco: one call restyles every live editor).
 */
export function applyMonacoTheme(monaco: MonacoApi, appTheme: "dark" | "light"): void {
  const root = document.querySelector("[data-theme]") ?? document.documentElement;
  const styles = getComputedStyle(root);
  const colors: Record<string, string> = {};
  const put = (key: string, token: string): void => {
    const v = tokenHex(styles, token);
    if (v) colors[key] = v;
  };
  put("editor.background", "--panel");
  put("editor.foreground", "--ink");
  put("editorGutter.background", "--panel");
  put("editorLineNumber.foreground", "--ink-4");
  put("editorLineNumber.activeForeground", "--ink-2");
  put("diffEditor.insertedTextBackground", "--code-add-bg");
  put("diffEditor.removedTextBackground", "--code-del-bg");
  const name = `orrery-${appTheme}`;
  monaco.editor.defineTheme(name, {
    base: appTheme === "dark" ? "vs-dark" : "vs",
    inherit: true,
    rules: [],
    colors,
  });
  monaco.editor.setTheme(name);
}

// ------------------------------------------------------------- languages ----

/**
 * langId() canonical tag → Monaco language. `load` imports the Monarch
 * contribution (static specifiers so each becomes a bundled lazy chunk); `id`
 * is the Monaco language id the model should be set to. Tags with no Monarch
 * grammar in `basic-languages` (haskell, erlang, nginx, wast, diff) render
 * plain — same graceful degradation as the CM loader.
 */
interface MonacoLang {
  id: string;
  load: () => Promise<unknown>;
}

const ML = (id: string, load: () => Promise<unknown>): MonacoLang => ({ id, load });

const LANGS: Record<string, MonacoLang> = {
  // TS tokenizer covers JS; langId collapses js/ts/jsx/tsx into one tag.
  javascript: ML("typescript", () =>
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution.js")),
  // no Monarch json grammar without the json language service — the TS
  // tokenizer highlights strings/numbers/braces correctly for JSON.
  json: ML("typescript", () =>
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution.js")),
  css: ML("css", () => import("monaco-editor/esm/vs/basic-languages/css/css.contribution.js")),
  sass: ML("scss", () => import("monaco-editor/esm/vs/basic-languages/scss/scss.contribution.js")),
  less: ML("less", () => import("monaco-editor/esm/vs/basic-languages/less/less.contribution.js")),
  html: ML("html", () => import("monaco-editor/esm/vs/basic-languages/html/html.contribution.js")),
  vue: ML("html", () => import("monaco-editor/esm/vs/basic-languages/html/html.contribution.js")),
  angular: ML("html", () => import("monaco-editor/esm/vs/basic-languages/html/html.contribution.js")),
  xml: ML("xml", () => import("monaco-editor/esm/vs/basic-languages/xml/xml.contribution.js")),
  markdown: ML("markdown", () =>
    import("monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution.js")),
  rust: ML("rust", () => import("monaco-editor/esm/vs/basic-languages/rust/rust.contribution.js")),
  python: ML("python", () =>
    import("monaco-editor/esm/vs/basic-languages/python/python.contribution.js")),
  java: ML("java", () => import("monaco-editor/esm/vs/basic-languages/java/java.contribution.js")),
  groovy: ML("java", () => import("monaco-editor/esm/vs/basic-languages/java/java.contribution.js")),
  yaml: ML("yaml", () => import("monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js")),
  sql: ML("sql", () => import("monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js")),
  cpp: ML("cpp", () => import("monaco-editor/esm/vs/basic-languages/cpp/cpp.contribution.js")),
  php: ML("php", () => import("monaco-editor/esm/vs/basic-languages/php/php.contribution.js")),
  go: ML("go", () => import("monaco-editor/esm/vs/basic-languages/go/go.contribution.js")),
  shell: ML("shell", () =>
    import("monaco-editor/esm/vs/basic-languages/shell/shell.contribution.js")),
  toml: ML("ini", () => import("monaco-editor/esm/vs/basic-languages/ini/ini.contribution.js")),
  properties: ML("ini", () => import("monaco-editor/esm/vs/basic-languages/ini/ini.contribution.js")),
  ruby: ML("ruby", () => import("monaco-editor/esm/vs/basic-languages/ruby/ruby.contribution.js")),
  csharp: ML("csharp", () =>
    import("monaco-editor/esm/vs/basic-languages/csharp/csharp.contribution.js")),
  kotlin: ML("kotlin", () =>
    import("monaco-editor/esm/vs/basic-languages/kotlin/kotlin.contribution.js")),
  scala: ML("scala", () =>
    import("monaco-editor/esm/vs/basic-languages/scala/scala.contribution.js")),
  dart: ML("dart", () => import("monaco-editor/esm/vs/basic-languages/dart/dart.contribution.js")),
  objectivec: ML("objective-c", () =>
    import("monaco-editor/esm/vs/basic-languages/objective-c/objective-c.contribution.js")),
  swift: ML("swift", () =>
    import("monaco-editor/esm/vs/basic-languages/swift/swift.contribution.js")),
  lua: ML("lua", () => import("monaco-editor/esm/vs/basic-languages/lua/lua.contribution.js")),
  perl: ML("perl", () => import("monaco-editor/esm/vs/basic-languages/perl/perl.contribution.js")),
  r: ML("r", () => import("monaco-editor/esm/vs/basic-languages/r/r.contribution.js")),
  clojure: ML("clojure", () =>
    import("monaco-editor/esm/vs/basic-languages/clojure/clojure.contribution.js")),
  julia: ML("julia", () =>
    import("monaco-editor/esm/vs/basic-languages/julia/julia.contribution.js")),
  powershell: ML("powershell", () =>
    import("monaco-editor/esm/vs/basic-languages/powershell/powershell.contribution.js")),
  dockerfile: ML("dockerfile", () =>
    import("monaco-editor/esm/vs/basic-languages/dockerfile/dockerfile.contribution.js")),
};

const loadedLangs = new Set<string>();

/**
 * Resolve a `langId()` tag to a registered Monaco language id, loading its
 * Monarch contribution on first use. Unknown tags (or a failed grammar chunk)
 * resolve to "plaintext" — the file still renders, just untokenized.
 */
export async function monacoLanguage(lang: string): Promise<string> {
  const def = LANGS[lang];
  if (!def) return "plaintext";
  if (!loadedLangs.has(def.id)) {
    try {
      await def.load();
      loadedLangs.add(def.id);
    } catch (e) {
      console.warn("[monaco-loader] grammar load failed for", lang, e);
      return "plaintext";
    }
  }
  return def.id;
}
