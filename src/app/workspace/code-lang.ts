import type { Extension } from "@codemirror/state";

// @lezer/common and @lezer/highlight are runtime-only (fetched from esm.sh, not
// installed as npm deps), so we can't `import` their types. Derive the shapes we
// touch from the installed @codemirror/language instead, and type the rest loosely.
type Highlighter = Parameters<typeof import("@codemirror/language")["syntaxHighlighting"]>[0];
interface LezerParser {
  parse(input: string): unknown;
}
interface LezerHighlight {
  classHighlighter: Highlighter;
  highlightCode(
    code: string,
    tree: unknown,
    highlighter: Highlighter,
    putText: (code: string, classes: string) => void,
    putBreak: () => void,
  ): void;
}

/**
 * Runtime loader for CodeMirror — nothing CodeMirror is bundled or kept in
 * package.json at runtime; the core editor AND the per-language grammars are
 * fetched from esm.sh on demand (first time a diff pane opens / a file of that
 * language is shown).
 *
 * The single-instance trap: CodeMirror checks extension identity (`instanceof`
 * against `@codemirror/state` objects), so every module — core + each grammar —
 * must share ONE copy of the low-level packages. The only thing that guarantees
 * that is an import map: it forces every reference (our top-level imports AND the
 * deep imports inside view/merge/grammars) to resolve to the SAME esm.sh URL.
 * `?deps` alone does not — esm.sh serves a separate build per entry point.
 *
 * We inject that import map from here (lazily, before the first CM import) rather
 * than hard-coding it in index.html, so the whole thing stays an on-demand JS
 * loader. Requires a modern Chromium/WebView (import maps added after load).
 */
const ESM = "https://esm.sh";
const VER: Record<string, string> = {
  "@codemirror/state": "6.6.0",
  "@codemirror/view": "6.43.0",
  "@codemirror/language": "6.12.3",
  "@codemirror/autocomplete": "6.20.3",
  "@codemirror/merge": "6.12.1",
  "@codemirror/theme-one-dark": "6.1.3",
  "@lezer/common": "1.5.2",
  "@lezer/highlight": "1.2.3",
  "@lezer/lr": "1.4.10",
};
// @lezer/highlight lives in SHARED (below) but also needs its OWN import-map
// entry so we can import it directly for standalone highlighting.
// the packages that must be shared (one instance) across core + every grammar
const SHARED = [
  "@codemirror/state",
  "@codemirror/view",
  "@codemirror/language",
  "@codemirror/autocomplete",
  "@lezer/common",
  "@lezer/highlight",
  "@lezer/lr",
];
const EXTERNAL = SHARED.join(",");

let mapInstalled = false;
/** Inject (once) an import map so every `@codemirror/*` / `@lezer/*` bare specifier
 *  — ours and the ones inside the esm.sh modules — resolves to a single URL. */
function ensureImportMap(): void {
  if (mapInstalled) return;
  mapInstalled = true;
  const imports: Record<string, string> = {};
  for (const [name, ver] of Object.entries(VER)) {
    // a module's own deps stay bare (?external) so they route back through this map
    const ext = SHARED.includes(name) || name === "@codemirror/merge" || name === "@codemirror/theme-one-dark";
    imports[name] = `${ESM}/${name}@${ver}` + (ext ? `?external=${EXTERNAL}` : "");
  }
  const script = document.createElement("script");
  script.type = "importmap";
  script.textContent = JSON.stringify({ imports });
  document.head.appendChild(script);
}

// import() of a runtime-computed specifier: the bundler can't analyze it, so it is
// left as a real runtime import resolved by the injected map / by the URL.
const importDyn = (spec: string): Promise<Record<string, unknown>> =>
  import(/* @vite-ignore */ spec) as Promise<Record<string, unknown>>;

/** The core CodeMirror modules the diff editor needs, loaded once and cached. */
export interface CMCore {
  state: typeof import("@codemirror/state");
  view: typeof import("@codemirror/view");
  language: typeof import("@codemirror/language");
  merge: typeof import("@codemirror/merge");
  themeOneDark: typeof import("@codemirror/theme-one-dark");
  highlight: LezerHighlight;
}

let corePromise: Promise<CMCore> | null = null;

/** Fetch (once) the core CodeMirror modules from esm.sh via the import map. */
export function loadCMCore(): Promise<CMCore> {
  if (!corePromise) {
    ensureImportMap();
    corePromise = Promise.all([
      importDyn("@codemirror/state"),
      importDyn("@codemirror/view"),
      importDyn("@codemirror/language"),
      importDyn("@codemirror/merge"),
      importDyn("@codemirror/theme-one-dark"),
      importDyn("@lezer/highlight"),
    ]).then(([state, view, language, merge, themeOneDark, highlight]) => ({
      state,
      view,
      language,
      merge,
      themeOneDark,
      highlight,
    })) as Promise<CMCore>;
    corePromise.catch(() => (corePromise = null)); // allow a retry on failure
  }
  return corePromise;
}

/**
 * Syntax-highlight theme. We point CodeMirror at Lezer's `classHighlighter`
 * (emits `tok-*` CLASSES instead of inline colors) so the SAME global `.tok-*`
 * IntelliJ-Darcula CSS colors both the CodeMirror diff AND the review-code diff
 * spans — one palette, identical everywhere, and theme (light/dark) switches for
 * free via the --tok-* CSS vars. `appTheme` is unused now (kept for the caller's
 * re-render-on-theme-change signature).
 */
export function buildTheme(cm: CMCore, _appTheme: "dark" | "light"): Extension {
  return [
    cm.view.EditorView.theme({
      "&": { color: "var(--ink-2)" },
      ".cm-gutters": { color: "var(--ink-4)" },
    }),
    cm.language.syntaxHighlighting(cm.highlight.classHighlighter),
  ];
}

/**
 * A grammar. `fn` is the export to use; for an official `@codemirror/lang-*`
 * package it is a factory returning a `LanguageSupport` (called with `arg`); for a
 * `@codemirror/legacy-modes` stream parser, `stream: true` wraps the exported
 * parser with our core `StreamLanguage`. Versions are major-pinned (esm.sh resolves
 * the latest 6.x) — only the SHARED deps need exact pins, which the import map gives.
 */
interface LangDef {
  pkg: string;
  fn: string;
  arg?: unknown;
  stream?: boolean;
}

const L = (pkg: string, fn: string, arg?: unknown): LangDef => ({ pkg: `@codemirror/lang-${pkg}@6`, fn, arg });
const S = (mode: string, fn: string): LangDef => ({ pkg: `@codemirror/legacy-modes@6/mode/${mode}`, fn, stream: true });

// keyed by the canonical tag from `langId()` in utils.ts
const LANGS: Record<string, LangDef> = {
  // official @codemirror/lang-* grammars
  javascript: L("javascript", "javascript", { typescript: true }),
  json: L("json", "json"),
  css: L("css", "css"),
  sass: L("sass", "sass"),
  less: L("less", "less"),
  html: L("html", "html"),
  xml: L("xml", "xml"),
  markdown: L("markdown", "markdown"),
  rust: L("rust", "rust"),
  python: L("python", "python"),
  java: L("java", "java"),
  yaml: L("yaml", "yaml"),
  sql: L("sql", "sql"),
  cpp: L("cpp", "cpp"),
  php: L("php", "php"),
  go: L("go", "go"),
  vue: L("vue", "vue"),
  wast: L("wast", "wast"),
  // @codemirror/legacy-modes (CM5 modes via StreamLanguage)
  shell: S("shell", "shell"),
  toml: S("toml", "toml"),
  ruby: S("ruby", "ruby"),
  csharp: S("clike", "csharp"),
  kotlin: S("clike", "kotlin"),
  scala: S("clike", "scala"),
  dart: S("clike", "dart"),
  objectivec: S("clike", "objectiveC"),
  swift: S("swift", "swift"),
  lua: S("lua", "lua"),
  perl: S("perl", "perl"),
  r: S("r", "r"),
  clojure: S("clojure", "clojure"),
  haskell: S("haskell", "haskell"),
  julia: S("julia", "julia"),
  erlang: S("erlang", "erlang"),
  powershell: S("powershell", "powerShell"),
  dockerfile: S("dockerfile", "dockerFile"),
  groovy: S("groovy", "groovy"),
  diff: S("diff", "diff"),
  properties: S("properties", "properties"),
  nginx: S("nginx", "nginx"),
};

const langCache = new Map<string, Promise<Extension>>();

/**
 * Resolve a language tag (from `langId(path)`) to its grammar extension, fetched
 * from esm.sh and cached per session. `?external` makes the grammar borrow our
 * shared CodeMirror instances via the import map. Unknown tags or a failed fetch
 * resolve to no highlighting (the diff still renders as plain text).
 */
export function loadLangExt(lang: string): Promise<Extension> {
  const def = LANGS[lang];
  if (!def) return Promise.resolve<Extension>([]);
  let p = langCache.get(lang);
  if (!p) {
    p = (async (): Promise<Extension> => {
      ensureImportMap();
      const m = await importDyn(`${ESM}/${def.pkg}?external=${EXTERNAL}`);
      const exp = m[def.fn];
      if (exp == null) throw new Error(`export "${def.fn}" missing from ${def.pkg}`);
      if (def.stream) {
        const cm = await loadCMCore();
        return new cm.language.LanguageSupport(cm.language.StreamLanguage.define(exp as never));
      }
      return (exp as (a?: unknown) => Extension)(def.arg);
    })().catch((e: unknown) => {
      console.warn("[code-lang] grammar load failed for", lang, e);
      langCache.delete(lang);
      return [] as Extension;
    });
    langCache.set(lang, p);
  }
  return p;
}

// ---- standalone (non-editor) syntax highlighting, for the review-code diff ----

/** One highlighted token: raw text plus its space-separated `tok-*` classes. */
export interface TokRun {
  s: string;
  cls: string;
}

const parserCache = new Map<string, Promise<LezerParser | null>>();

/** Resolve a language tag to its Lezer parser (for standalone highlighting),
 *  mirroring {@link loadLangExt}. Unknown/failed → null (caller renders plain). */
export function loadParser(lang: string): Promise<LezerParser | null> {
  const def = LANGS[lang];
  if (!def) return Promise.resolve(null);
  let p = parserCache.get(lang);
  if (!p) {
    p = (async (): Promise<LezerParser | null> => {
      ensureImportMap();
      const m = await importDyn(`${ESM}/${def.pkg}?external=${EXTERNAL}`);
      const exp = m[def.fn];
      if (exp == null) return null;
      let support: unknown;
      if (def.stream) {
        const cm = await loadCMCore();
        support = new cm.language.LanguageSupport(cm.language.StreamLanguage.define(exp as never));
      } else {
        support = (exp as (a?: unknown) => unknown)(def.arg);
      }
      // LanguageSupport → .language.parser (Language). Guard shape defensively.
      const language = (support as { language?: { parser?: LezerParser } })?.language;
      return language?.parser ?? null;
    })().catch((e: unknown) => {
      console.warn("[code-lang] parser load failed for", lang, e);
      parserCache.delete(lang);
      return null;
    });
    parserCache.set(lang, p);
  }
  return p;
}

/**
 * Highlight ONE line of `code` in `lang` into `tok-*`-classed runs (via the same
 * Lezer `classHighlighter` CodeMirror uses, so colors match). Returns null when
 * the language is unknown/unavailable — the caller then renders plain text.
 * Per-line (no cross-line state) is fine for a diff view and keeps it simple.
 */
export async function highlightRuns(lang: string, code: string): Promise<TokRun[] | null> {
  if (!code) return [];
  const parser = await loadParser(lang);
  if (!parser) return null;
  const cm = await loadCMCore();
  const { highlightCode, classHighlighter } = cm.highlight;
  const tree = parser.parse(code);
  const runs: TokRun[] = [];
  highlightCode(
    code,
    tree,
    classHighlighter,
    (text: string, cls: string) => runs.push({ s: text, cls }),
    () => runs.push({ s: "\n", cls: "" }),
  );
  return runs;
}
