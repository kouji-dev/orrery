import type * as monacoApi from "monaco-editor";

import { DocSymbol, LibHint, NavHover, NavLocation, NavResult } from "../models";

/**
 * Monaco ⇄ backend adapters for symbol navigation (M2). Pure: every function
 * takes the Monaco namespace as a parameter (or a narrow slice of it), so the
 * spec drives them with a fake. The Angular glue — invokes, model loading,
 * registration once per language — lives in nav-providers.service.ts.
 *
 * Coordinates: the backend is 0-based everywhere, Monaco is 1-based. The
 * conversion happens HERE and nowhere else.
 */

export const MODEL_SCHEME = "orrery";
/** `orrery-diff://<id>~<n>/<path>` — the model uri of the NEW side of a
 *  diff surface (the agent's change view, the git tool window). Same id +
 *  path as the worktree file so hover / Ctrl+click / references route to the
 *  same backend lookups (user, 2026-09-15: navigation must work in diffs,
 *  not only in open files), but its own scheme + a serial so it never
 *  collides with the editor's model of that file or with a second diff of
 *  it. The serial rides in the authority behind `~` (unreserved, so neither
 *  Monaco's `Uri.toString()` nor a percent-decoder touches it; a query
 *  string would be re-encoded on the way through). The OLD side stays
 *  anonymous: its text is not what is on disk. */
export const DIFF_MODEL_SCHEME = "orrery-diff";
let diffSeq = 0;
export function diffModelUri(id: string, path: string): string {
  const p = path.replace(/%/g, "%25").replace(/#/g, "%23").replace(/\?/g, "%3F");
  return `${DIFF_MODEL_SCHEME}://${id}~${++diffSeq}/${p}`;
}

/** `orrery://<id>/<path>` — the model uri of a worktree file. `#`, `?` and
 *  `%` are the three characters Uri parsing would misread as query/fragment
 *  or a stray escape; everything else (spaces included) round-trips through
 *  `Uri.toString()` percent-encoding + `parseModelUri` decoding. */
export function modelUri(id: string, path: string): string {
  const p = path.replace(/%/g, "%25").replace(/#/g, "%23").replace(/\?/g, "%3F");
  return `${MODEL_SCHEME}://${id}/${p}`;
}

/** Inverse of `modelUri` for any spelling of it (raw or percent-encoded, as
 *  `Uri.toString()` emits). Null for every other scheme — those are virtual
 *  read-only docs (M4) and never map to a worktree file. */
export function parseModelUri(uri: string): { id: string; path: string } | null {
  const m = /^orrery(?:-diff)?:\/\/([^/?#~]+)(?:~[^/?#]*)?\/([^?#]*)$/.exec(uri);
  if (!m) return null;
  return { id: safeDecode(m[1]), path: safeDecode(m[2]) };
}

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** Whether a tab path is a virtual read-only document (any scheme but the
 *  worktree's own `orrery://`) rather than a worktree-relative path. */
export function isVirtualUri(p: string): boolean {
  return p.includes("://") && !p.startsWith(MODEL_SCHEME + "://") && !p.startsWith(DIFF_MODEL_SCHEME + "://");
}

/** A diff surface's model: its text is what the user is looking at, which
 *  may differ from the worktree file — so lookups always carry it. */
function isDiffModel(model: monacoApi.editor.ITextModel): boolean {
  return model.uri.toString().startsWith(DIFF_MODEL_SCHEME + ":");
}

/** Raw virtual uris by their Monaco spelling: `Uri.parse(raw).toString()`
 *  re-encodes, and the opener must hand `nav_virtual_read` the exact string
 *  the server answered with (a jdtls uri carries a `?=` query the server
 *  parses itself). */
const VIRTUAL_RAW = new Map<string, string>();

/** The raw uri a Monaco resource string stands for, if a location remembered it. */
export function rawVirtualUri(monacoUri: string): string | undefined {
  return VIRTUAL_RAW.get(monacoUri);
}

/** What a virtual location stands for, by raw AND Monaco spelling: a
 *  library hit's `preview` (the fully-qualified name, "java.util.ArrayList")
 *  or its `container`. Monaco's peek tree has no per-location detail slot
 *  (it previews the model line), so this feeds the app's own chrome — the
 *  read-only tab's toolbar tooltip — instead. */
const VIRTUAL_DETAIL = new Map<string, string>();

export function locationDetail(uri: string): string | undefined {
  return VIRTUAL_DETAIL.get(uri);
}

/** The Monaco model uri of a location: the backend's, else rebuilt from
 *  `id` + `path` (both null for a library hit — then only `uri` counts). */
export function locationUri(loc: NavLocation): string {
  return loc.uri || modelUri(loc.id ?? "", loc.path ?? "");
}

/** The narrow Monaco slice the adapters build values with. */
export interface NavMonaco {
  Uri: { parse(v: string): monacoApi.Uri };
  Range: new (sl: number, sc: number, el: number, ec: number) => monacoApi.Range;
  languages: { SymbolKind: typeof monacoApi.languages.SymbolKind };
}

/** A backend location → a Monaco `Location` (1-based range). The uri the
 *  backend sends wins; `id`+`path` rebuild it when it is missing. */
export function toLocation(monaco: NavMonaco, loc: NavLocation): monacoApi.languages.Location {
  const raw = locationUri(loc);
  const uri = monaco.Uri.parse(raw);
  if (isVirtualUri(raw)) {
    const key = uri.toString();
    VIRTUAL_RAW.set(key, raw);
    const detail = loc.preview || loc.container;
    if (detail) {
      VIRTUAL_DETAIL.set(key, detail);
      VIRTUAL_DETAIL.set(raw, detail);
    }
  }
  return {
    uri,
    range: new monaco.Range(loc.line + 1, loc.col + 1, loc.endLine + 1, loc.endCol + 1),
  };
}

/** Hits in the CURRENT root first: a symbol declared in this worktree and
 *  also in three sibling worktrees should jump, not peek. Nothing local →
 *  every hit, so cross-root results still show up. */
export function pickLocations(locs: NavLocation[], currentId: string): NavLocation[] {
  const same = locs.filter((l) => l.id === currentId);
  return same.length ? same : locs;
}

/** Grammar kind names (tags.scm captures + the design's SYM_KIND_ICON keys)
 *  → Monaco SymbolKind. Unknown → Variable. */
export function symbolKind(monaco: NavMonaco, kind: string | null | undefined): monacoApi.languages.SymbolKind {
  const K = monaco.languages.SymbolKind;
  switch ((kind ?? "").toLowerCase()) {
    case "class":
      return K.Class;
    case "interface":
    case "trait":
    case "protocol":
      return K.Interface;
    case "fn":
    case "func":
    case "function":
    case "def":
    case "macro":
      return K.Function;
    case "method":
      return K.Method;
    case "constructor":
    case "ctor":
      return K.Constructor;
    case "field":
      return K.Field;
    case "property":
    case "prop":
      return K.Property;
    case "enum":
      return K.Enum;
    case "enummember":
    case "variant":
      return K.EnumMember;
    case "struct":
      return K.Struct;
    case "type":
    case "typedef":
    case "alias":
    case "typealias":
      return K.TypeParameter;
    case "module":
    case "mod":
      return K.Module;
    case "namespace":
    case "impl":
      return K.Namespace;
    case "package":
      return K.Package;
    case "const":
    case "constant":
    case "static":
      return K.Constant;
    default:
      return K.Variable;
  }
}

/** One outline node → Monaco `DocumentSymbol` (recursive). The selection
 *  range is the name at its `sel*` anchor, clamped into the full range. */
export function toDocumentSymbol(monaco: NavMonaco, s: DocSymbol): monacoApi.languages.DocumentSymbol {
  const range = new monaco.Range(s.line + 1, s.col + 1, s.endLine + 1, s.endCol + 1);
  let selectionRange = new monaco.Range(s.selLine + 1, s.selCol + 1, s.selLine + 1, s.selCol + 1 + s.name.length);
  if (!range.containsRange(selectionRange)) selectionRange = range;
  return {
    name: s.name,
    detail: s.container ?? "",
    kind: symbolKind(monaco, s.kind),
    tags: [],
    range,
    selectionRange,
    children: (s.children ?? []).map((c) => toDocumentSymbol(monaco, c)),
  };
}

/** The hover card body. `contents` is already markdown; a bare signature
 *  (no fence) is wrapped in one for the language. The source badge line is
 *  what NavHoverCard shows bottom-right — "index" or "language server". */
export function hoverMarkdown(h: NavHover, langId: string): string {
  const body = h.contents.trim();
  if (!body) return "";
  const sig = body.includes("```") ? body : "```" + langId + "\n" + body + "\n```";
  return sig + "\n\n`" + (h.source === "lsp" ? "language server" : "index") + "`";
}

/** What the providers need from the app. Line/col are 0-based (backend). */
export interface NavDeps {
  definition(id: string, path: string, line: number, col: number, word: string, text?: string): Promise<NavResult>;
  references(
    id: string,
    path: string,
    line: number,
    col: number,
    word: string,
    includeDeclaration: boolean,
    text?: string,
  ): Promise<NavResult>;
  hover(id: string, path: string, line: number, col: number, word: string): Promise<NavHover | null>;
  documentSymbols(id: string, path: string, text: string): Promise<DocSymbol[]>;
  /** Give a peek target a model so Monaco can preview it (`getModel(uri)`). */
  ensureModel(uri: string): Promise<unknown>;
  /** Should the request carry the buffer text (i.e. is it dirty on disk)? */
  sendText?(id: string, path: string): boolean;
  /** A definition request came back empty — the editor shows its NavHint;
   *  `hint` (M4) says why when the library index knows (no JDK, a sources
   *  jar not downloaded). */
  onNoDefinition?(id: string, path: string, hint: LibHint | null): void;
  /** The answer came from the index because the server for this language
   *  was still starting — the editor shows the `fallback` NavHint (M3). */
  onFallback?(id: string, path: string): void;
}

export interface NavProviders {
  definition: monacoApi.languages.DefinitionProvider;
  references: monacoApi.languages.ReferenceProvider;
  documentSymbol: monacoApi.languages.DocumentSymbolProvider;
  hover: monacoApi.languages.HoverProvider;
}

type Tok = monacoApi.CancellationToken;

/**
 * The four providers. Each keeps its own generation counter: a newer call
 * bumps it, a late answer from an older one returns null instead of landing
 * on the wrong request (on top of Monaco's own cancellation token, which
 * the backend cannot see).
 */
export function buildProviders(monaco: NavMonaco, deps: NavDeps): NavProviders {
  const gens = { def: 0, ref: 0, hov: 0, sym: 0 };

  function target(model: monacoApi.editor.ITextModel, position: monacoApi.Position) {
    const t = parseModelUri(model.uri.toString());
    if (!t) return null;
    const w = model.getWordAtPosition(position);
    if (!w) return null;
    return { ...t, word: w.word, wordRange: new monaco.Range(position.lineNumber, w.startColumn, position.lineNumber, w.endColumn) };
  }

  const textFor = (model: monacoApi.editor.ITextModel, id: string, path: string) =>
    isDiffModel(model) || deps.sendText?.(id, path) ? model.getValue() : undefined;

  async function ensureAll(locs: NavLocation[]): Promise<void> {
    await Promise.all(locs.map((l) => deps.ensureModel(locationUri(l)).catch(() => null)));
  }

  return {
    definition: {
      async provideDefinition(model, position, token: Tok) {
        const t = target(model, position);
        if (!t) return null;
        const gen = ++gens.def;
        const res = await deps.definition(t.id, t.path, position.lineNumber - 1, position.column - 1, t.word, textFor(model, t.id, t.path));
        if (gen !== gens.def || token.isCancellationRequested) return null;
        const locs = pickLocations(res.locations, t.id);
        if (!locs.length) {
          deps.onNoDefinition?.(t.id, t.path, res.libHint ?? null);
          return [];
        }
        if (res.lspState === "starting") deps.onFallback?.(t.id, t.path);
        // one hit goes straight to the opener; several open a peek, and the
        // peek previews only what already has a model
        if (locs.length > 1) await ensureAll(locs);
        if (gen !== gens.def || token.isCancellationRequested) return null;
        return locs.map((l) => toLocation(monaco, l));
      },
    },
    references: {
      async provideReferences(model, position, context, token: Tok) {
        const t = target(model, position);
        if (!t) return null;
        const gen = ++gens.ref;
        const res = await deps.references(
          t.id,
          t.path,
          position.lineNumber - 1,
          position.column - 1,
          t.word,
          context.includeDeclaration,
          textFor(model, t.id, t.path),
        );
        if (gen !== gens.ref || token.isCancellationRequested) return null;
        await ensureAll(res.locations);
        if (gen !== gens.ref || token.isCancellationRequested) return null;
        return res.locations.map((l) => toLocation(monaco, l));
      },
    },
    documentSymbol: {
      displayName: "orrery",
      async provideDocumentSymbols(model, token: Tok) {
        const t = parseModelUri(model.uri.toString());
        if (!t) return null;
        const gen = ++gens.sym;
        const syms = await deps.documentSymbols(t.id, t.path, model.getValue());
        if (gen !== gens.sym || token.isCancellationRequested) return null;
        return syms.map((s) => toDocumentSymbol(monaco, s));
      },
    },
    hover: {
      async provideHover(model, position, token: Tok) {
        const t = target(model, position);
        if (!t) return null;
        const gen = ++gens.hov;
        const h = await deps.hover(t.id, t.path, position.lineNumber - 1, position.column - 1, t.word);
        if (gen !== gens.hov || token.isCancellationRequested || !h) return null;
        const md = hoverMarkdown(h, model.getLanguageId());
        if (!md) return null;
        if (h.lspState === "starting") deps.onFallback?.(t.id, t.path);
        const range = h.range
          ? new monaco.Range(h.range.line + 1, h.range.col + 1, h.range.endLine + 1, h.range.endCol + 1)
          : t.wordRange;
        return { range, contents: [{ value: md }] };
      },
    },
  };
}
