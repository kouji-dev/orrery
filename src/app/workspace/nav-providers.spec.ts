import { describe, expect, it, vi } from "vitest";
import type * as monacoApi from "monaco-editor";

import { DocSymbol, NavLocation, NavResult } from "../models";
import {
  buildProviders,
  hoverMarkdown,
  isVirtualUri,
  modelUri,
  NavDeps,
  NavMonaco,
  parseModelUri,
  pickLocations,
  locationDetail,
  locationUri,
  rawVirtualUri,
  symbolKind,
  toDocumentSymbol,
  toLocation,
} from "./nav-providers";

// Just enough of the Monaco namespace (the fakeMonaco pattern): a Uri that
// percent-encodes like the real one, a Range with containsRange, SymbolKind.
class FakeRange {
  constructor(
    public startLineNumber: number,
    public startColumn: number,
    public endLineNumber: number,
    public endColumn: number,
  ) {}
  containsRange(r: FakeRange): boolean {
    if (r.startLineNumber < this.startLineNumber || r.endLineNumber > this.endLineNumber) return false;
    if (r.startLineNumber === this.startLineNumber && r.startColumn < this.startColumn) return false;
    if (r.endLineNumber === this.endLineNumber && r.endColumn > this.endColumn) return false;
    return true;
  }
}
// Uri.parse percent-DEcodes the path, toString() re-encodes it (the real
// class does both), so a `%23` in never becomes a `%2523` out.
const fakeUri = (v: string) => ({
  raw: v,
  toString: () => {
    const m = /^([\w-]+):\/\/([^/]+)\/(.*)$/.exec(v);
    if (!m) return v;
    const path = m[3]
      .split("/")
      .map((seg) => encodeURIComponent(decodeURIComponent(seg)))
      .join("/");
    return `${m[1]}://${m[2]}/${path}`;
  },
});
const SymbolKind = {
  File: 0, Module: 1, Namespace: 2, Package: 3, Class: 4, Method: 5, Property: 6, Field: 7, Constructor: 8,
  Enum: 9, Interface: 10, Function: 11, Variable: 12, Constant: 13, String: 14, Number: 15, Boolean: 16,
  Array: 17, Object: 18, Key: 19, Null: 20, EnumMember: 21, Struct: 22, Event: 23, Operator: 24, TypeParameter: 25,
};
const monaco = {
  Uri: { parse: fakeUri },
  Range: FakeRange,
  languages: { SymbolKind },
} as unknown as NavMonaco;

const loc = (over: Partial<NavLocation> & { id: string; path: string }): NavLocation => ({
  uri: modelUri(over.id, over.path),
  line: 0,
  col: 0,
  endLine: 0,
  endCol: 0,
  ...over,
});

describe("model uri", () => {
  it("round-trips id + path, including spaces and a hash, through Uri encoding", () => {
    const path = "src/dir with space/we#ird?.ts";
    const uri = modelUri("agent-1", path);
    expect(uri.startsWith("orrery://agent-1/")).toBe(true);
    expect(parseModelUri(uri)).toEqual({ id: "agent-1", path });
    // what Monaco hands back after Uri.parse(...).toString()
    expect(parseModelUri(fakeUri(uri).toString())).toEqual({ id: "agent-1", path });
  });

  it("rejects every other scheme (virtual docs are M4's business)", () => {
    expect(parseModelUri("orrery-lib://jdk/java/util/List.java")).toBeNull();
    expect(parseModelUri("jdt://contents/x.class")).toBeNull();
    expect(parseModelUri("inmemory://model/1")).toBeNull();
    expect(isVirtualUri("orrery-lib://jdk/x")).toBe(true);
    expect(isVirtualUri("jdt://contents/x.class")).toBe(true);
    expect(isVirtualUri("src/app.ts")).toBe(false);
    // the worktree's own scheme is NOT virtual
    expect(isVirtualUri("orrery://agent-1/src/app.ts")).toBe(false);
  });
});

describe("pickLocations", () => {
  it("prefers hits in the current root, else keeps them all", () => {
    const a = loc({ id: "a", path: "x.ts" });
    const b = loc({ id: "b", path: "x.ts" });
    const c = loc({ id: "c", path: "y.ts" });
    expect(pickLocations([a, b, c], "b")).toEqual([b]);
    expect(pickLocations([a, b, c], "zzz")).toEqual([a, b, c]);
    expect(pickLocations([], "a")).toEqual([]);
  });
});

describe("toLocation", () => {
  it("maps a 0-based backend location to a 1-based Monaco range on the model uri", () => {
    const l = toLocation(monaco, loc({ id: "a", path: "src/x.ts", line: 4, col: 2, endLine: 4, endCol: 9 }));
    expect((l.uri as unknown as { raw: string }).raw).toBe("orrery://a/src/x.ts");
    expect(l.range).toMatchObject({ startLineNumber: 5, startColumn: 3, endLineNumber: 5, endColumn: 10 });
  });

  it("rebuilds the uri from id + path when the backend sent none", () => {
    const l = toLocation(monaco, loc({ id: "a", path: "b.rs", uri: "" }));
    expect((l.uri as unknown as { raw: string }).raw).toBe("orrery://a/b.rs");
  });

  it("remembers a virtual uri's raw spelling under its Monaco spelling (M3 opener)", () => {
    const raw = "jdt://contents/java.util/List.class?=p/%5C/x%5C/y.jar";
    const l = toLocation(monaco, loc({ id: "", path: "", uri: raw }));
    const key = l.uri.toString();
    expect(rawVirtualUri(key)).toBe(raw);
    // worktree uris are never remembered
    const w = toLocation(monaco, loc({ id: "a", path: "b.rs" }));
    expect(rawVirtualUri(w.uri.toString())).toBeUndefined();
  });

  it("a library hit (id/path null) keeps its uri and remembers the fq name as the detail (M4)", () => {
    const raw = "orrery-lib://jdk1/java.base/java/util/ArrayList.java";
    const hit: NavLocation = { uri: raw, id: null, path: null, line: 2, col: 13, endLine: 2, endCol: 22, kind: "class", preview: "java.util.ArrayList" };
    expect(locationUri(hit)).toBe(raw);
    const l = toLocation(monaco, hit);
    expect((l.uri as unknown as { raw: string }).raw).toBe(raw);
    expect(l.range).toMatchObject({ startLineNumber: 3, startColumn: 14 });
    // by both spellings: the opener sees Monaco's, the tab holds the raw one
    expect(locationDetail(l.uri.toString())).toBe("java.util.ArrayList");
    expect(locationDetail(raw)).toBe("java.util.ArrayList");
    expect(rawVirtualUri(l.uri.toString())).toBe(raw);
    // the container stands in when there is no preview; worktree hits carry none
    const c = toLocation(monaco, { ...hit, uri: "orrery-lib://jdk1/java.base/java/util/List.java", preview: null, container: "java.util" });
    expect(locationDetail(c.uri.toString())).toBe("java.util");
    const w = toLocation(monaco, loc({ id: "a", path: "b.rs", preview: "x" }));
    expect(locationDetail(w.uri.toString())).toBeUndefined();
    // a library hit never counts as "in the current root"
    expect(pickLocations([hit, loc({ id: "a", path: "b.rs" })], "a")).toHaveLength(1);
    expect(pickLocations([hit], "a")).toEqual([hit]);
  });
});

describe("symbolKind", () => {
  it("maps grammar kinds and defaults to Variable", () => {
    expect(symbolKind(monaco, "class")).toBe(SymbolKind.Class);
    expect(symbolKind(monaco, "fn")).toBe(SymbolKind.Function);
    expect(symbolKind(monaco, "method")).toBe(SymbolKind.Method);
    expect(symbolKind(monaco, "field")).toBe(SymbolKind.Field);
    expect(symbolKind(monaco, "Enum")).toBe(SymbolKind.Enum);
    expect(symbolKind(monaco, "struct")).toBe(SymbolKind.Struct);
    expect(symbolKind(monaco, "const")).toBe(SymbolKind.Constant);
    expect(symbolKind(monaco, "whatever")).toBe(SymbolKind.Variable);
    expect(symbolKind(monaco, null)).toBe(SymbolKind.Variable);
  });
});

describe("toDocumentSymbol", () => {
  const sym = (over: Partial<DocSymbol> & { name: string }): DocSymbol => ({
    kind: "fn",
    line: 0,
    col: 0,
    endLine: 0,
    endCol: 10,
    selLine: 0,
    selCol: 0,
    children: [],
    ...over,
  });

  it("maps a nested tree, 1-based, with the name as the selection range", () => {
    const d = toDocumentSymbol(
      monaco,
      sym({
        name: "Foo",
        kind: "class",
        line: 2,
        col: 0,
        endLine: 10,
        endCol: 1,
        selLine: 2,
        selCol: 6,
        children: [sym({ name: "bar", kind: "method", line: 3, col: 2, endLine: 5, endCol: 3, selLine: 3, selCol: 2, container: "Foo" })],
      }),
    );
    expect(d.name).toBe("Foo");
    expect(d.kind).toBe(SymbolKind.Class);
    expect(d.range).toMatchObject({ startLineNumber: 3, startColumn: 1, endLineNumber: 11, endColumn: 2 });
    expect(d.selectionRange).toMatchObject({ startLineNumber: 3, startColumn: 7, endLineNumber: 3, endColumn: 10 });
    expect(d.children).toHaveLength(1);
    expect(d.children![0]).toMatchObject({ name: "bar", detail: "Foo", kind: SymbolKind.Method });
    expect(d.children![0].range).toMatchObject({ startLineNumber: 4, startColumn: 3 });
  });

  it("falls back to the full range when the selection anchor sits outside it", () => {
    const d = toDocumentSymbol(monaco, sym({ name: "x", line: 5, endLine: 6, endCol: 3, selLine: 1, selCol: 0 }));
    expect(d.selectionRange).toBe(d.range);
  });
});

describe("hoverMarkdown", () => {
  it("fences a bare signature for the language and appends the source badge", () => {
    const md = hoverMarkdown({ source: "index", contents: "fn parse(s: &str) -> Ast" }, "rust");
    expect(md).toBe("```rust\nfn parse(s: &str) -> Ast\n```\n\n`index`");
  });

  it("keeps ready-made markdown as is, badge says language server for lsp", () => {
    const body = "```java\nList<String> names\n```\nfield · Foo\nsrc/Foo.java";
    const md = hoverMarkdown({ source: "lsp", contents: body }, "java");
    expect(md.startsWith(body)).toBe(true);
    expect(md.endsWith("`language server`")).toBe(true);
  });

  it("is empty for empty contents (no card at all)", () => {
    expect(hoverMarkdown({ source: "index", contents: "  " }, "ts")).toBe("");
  });
});

describe("buildProviders", () => {
  function model(uri: string, word = "foo", value = "let foo = 1;") {
    return {
      uri: fakeUri(uri),
      getWordAtPosition: () => ({ word, startColumn: 5, endColumn: 8 }),
      getValue: () => value,
      getLanguageId: () => "typescript",
    } as unknown as monacoApi.editor.ITextModel;
  }
  const pos = { lineNumber: 3, column: 6 } as monacoApi.Position;
  const live = { isCancellationRequested: false } as monacoApi.CancellationToken;
  const result = (locations: NavLocation[]): NavResult => ({ source: "index", locations, lspState: null });

  function deps(over: Partial<NavDeps> = {}): NavDeps & { ensureModel: ReturnType<typeof vi.fn> } {
    return {
      definition: vi.fn(async () => result([])),
      references: vi.fn(async () => result([])),
      hover: vi.fn(async () => null),
      documentSymbols: vi.fn(async () => []),
      ensureModel: vi.fn(async () => null),
      ...over,
    } as NavDeps & { ensureModel: ReturnType<typeof vi.fn> };
  }

  it("asks the backend in 0-based coordinates and answers 1-based locations", async () => {
    const d = deps({
      definition: vi.fn(async () => result([loc({ id: "a", path: "src/def.ts", line: 9, col: 4, endLine: 9, endCol: 7 })])),
      sendText: () => false,
    });
    const p = buildProviders(monaco, d);
    const out = await p.definition.provideDefinition(model("orrery://a/src/use.ts"), pos, live);
    expect(d.definition).toHaveBeenCalledWith("a", "src/use.ts", 2, 5, "foo", undefined);
    expect(out).toHaveLength(1);
    expect((out as monacoApi.languages.Location[])[0].range).toMatchObject({ startLineNumber: 10, startColumn: 5 });
    // a single target needs no model of its own — the opener takes it
    expect(d.ensureModel).not.toHaveBeenCalled();
  });

  it("sends the buffer text only when the file is dirty", async () => {
    const d = deps({ sendText: () => true });
    const p = buildProviders(monaco, d);
    await p.definition.provideDefinition(model("orrery://a/src/use.ts", "foo", "TEXT"), pos, live);
    expect(d.definition).toHaveBeenCalledWith("a", "src/use.ts", 2, 5, "foo", "TEXT");
  });

  it("ensures a model per target when several locations will peek", async () => {
    const locs = [loc({ id: "a", path: "x.ts" }), loc({ id: "a", path: "y.ts" })];
    const d = deps({ definition: vi.fn(async () => result(locs)) });
    const p = buildProviders(monaco, d);
    const out = await p.definition.provideDefinition(model("orrery://a/use.ts"), pos, live);
    expect(out).toHaveLength(2);
    expect(d.ensureModel).toHaveBeenCalledTimes(2);
    expect(d.ensureModel).toHaveBeenCalledWith("orrery://a/x.ts");
  });

  it("reports an empty definition through onNoDefinition and returns []", async () => {
    const onNoDefinition = vi.fn();
    const p = buildProviders(monaco, deps({ onNoDefinition }));
    const out = await p.definition.provideDefinition(model("orrery://a/use.ts"), pos, live);
    expect(out).toEqual([]);
    expect(onNoDefinition).toHaveBeenCalledWith("a", "use.ts", null);
  });

  it("M4: an empty answer carries the backend's libHint (jdk-missing / sources-missing) to onNoDefinition", async () => {
    for (const hint of ["jdk-missing", "sources-missing"] as const) {
      const onNoDefinition = vi.fn();
      const d = deps({ definition: vi.fn(async () => ({ ...result([]), libHint: hint })), onNoDefinition });
      expect(await buildProviders(monaco, d).definition.provideDefinition(model("orrery://a/A.java"), pos, live)).toEqual([]);
      expect(onNoDefinition).toHaveBeenCalledWith("a", "A.java", hint);
    }
    // a hint on a NON-empty answer is ignored
    const onNoDefinition = vi.fn();
    const d = deps({ definition: vi.fn(async () => ({ ...result([loc({ id: "a", path: "def.ts" })]), libHint: "jdk-missing" as const })), onNoDefinition });
    expect(await buildProviders(monaco, d).definition.provideDefinition(model("orrery://a/use.ts"), pos, live)).toHaveLength(1);
    expect(onNoDefinition).not.toHaveBeenCalled();
  });

  it("M3 fallback: an lspState of 'starting' on a NON-empty answer raises onFallback, keeps the index hits", async () => {
    const onFallback = vi.fn();
    const onNoDefinition = vi.fn();
    const d = deps({
      definition: vi.fn(async () => ({ ...result([loc({ id: "a", path: "def.ts" })]), lspState: "starting" as const })),
      onFallback,
      onNoDefinition,
    });
    const out = await buildProviders(monaco, d).definition.provideDefinition(model("orrery://a/use.ts"), pos, live);
    expect(out).toHaveLength(1);
    expect(onFallback).toHaveBeenCalledWith("a", "use.ts");
    expect(onNoDefinition).not.toHaveBeenCalled();
  });

  it("M3 fallback: an empty answer while starting is 'no definition', not a fallback", async () => {
    const onFallback = vi.fn();
    const onNoDefinition = vi.fn();
    const d = deps({ definition: vi.fn(async () => ({ ...result([]), lspState: "starting" as const })), onFallback, onNoDefinition });
    expect(await buildProviders(monaco, d).definition.provideDefinition(model("orrery://a/use.ts"), pos, live)).toEqual([]);
    expect(onFallback).not.toHaveBeenCalled();
    expect(onNoDefinition).toHaveBeenCalledWith("a", "use.ts", null);
  });

  it("M3 fallback: a 'timeout' / null lspState never raises it; a hover while starting does", async () => {
    const onFallback = vi.fn();
    const d = deps({
      definition: vi.fn(async () => ({ ...result([loc({ id: "a", path: "def.ts" })]), lspState: "timeout" as const })),
      hover: vi.fn(async () => ({ source: "index" as const, contents: "x", lspState: "starting" as const })),
      onFallback,
    });
    const p = buildProviders(monaco, d);
    await p.definition.provideDefinition(model("orrery://a/use.ts"), pos, live);
    expect(onFallback).not.toHaveBeenCalled();
    await p.hover.provideHover(model("orrery://a/use.ts"), pos, live);
    expect(onFallback).toHaveBeenCalledWith("a", "use.ts");
  });

  it("drops a cancelled request and a stale one (a newer call bumped the generation)", async () => {
    let release!: (r: NavResult) => void;
    const first = new Promise<NavResult>((r) => (release = r));
    const d = deps({
      definition: vi
        .fn<NavDeps["definition"]>()
        .mockImplementationOnce(() => first)
        .mockImplementationOnce(async () => result([loc({ id: "a", path: "b.ts" })])),
    });
    const p = buildProviders(monaco, d);
    const m = model("orrery://a/use.ts");
    const stale = p.definition.provideDefinition(m, pos, live);
    const fresh = await p.definition.provideDefinition(m, pos, live);
    expect(fresh).toHaveLength(1);
    release(result([loc({ id: "a", path: "old.ts" })]));
    expect(await stale).toBeNull();

    const cancelled = { isCancellationRequested: true } as monacoApi.CancellationToken;
    expect(await p.hover.provideHover(m, pos, cancelled)).toBeNull();
  });

  it("ignores models outside the orrery scheme and positions without a word", async () => {
    const d = deps();
    const p = buildProviders(monaco, d);
    expect(await p.definition.provideDefinition(model("inmemory://model/1"), pos, live)).toBeNull();
    const noWord = { ...model("orrery://a/x.ts"), getWordAtPosition: () => null } as unknown as monacoApi.editor.ITextModel;
    expect(await p.hover.provideHover(noWord, pos, live)).toBeNull();
    expect(d.definition).not.toHaveBeenCalled();
    expect(d.hover).not.toHaveBeenCalled();
  });

  it("hover: markdown card over the word range (or the backend's range)", async () => {
    const d = deps({ hover: vi.fn(async () => ({ source: "index" as const, contents: "const foo: number" })) });
    const p = buildProviders(monaco, d);
    const h = await p.hover.provideHover(model("orrery://a/x.ts"), pos, live);
    expect(h?.contents[0].value).toBe("```typescript\nconst foo: number\n```\n\n`index`");
    expect(h?.range).toMatchObject({ startLineNumber: 3, startColumn: 5, endColumn: 8 });

    const ranged = deps({
      hover: vi.fn(async () => ({ source: "index" as const, contents: "x", range: { line: 1, col: 1, endLine: 1, endCol: 4 } })),
    });
    const h2 = await buildProviders(monaco, ranged).hover.provideHover(model("orrery://a/x.ts"), pos, live);
    expect(h2?.range).toMatchObject({ startLineNumber: 2, startColumn: 2, endLineNumber: 2, endColumn: 5 });
  });

  it("references pass includeDeclaration and ensure every target", async () => {
    const locs = [loc({ id: "a", path: "x.ts" }), loc({ id: "b", path: "y.ts" })];
    const d = deps({ references: vi.fn(async () => result(locs)) });
    const p = buildProviders(monaco, d);
    const out = await p.references.provideReferences(
      model("orrery://a/use.ts"),
      pos,
      { includeDeclaration: true },
      live,
    );
    expect(d.references).toHaveBeenCalledWith("a", "use.ts", 2, 5, "foo", true, undefined);
    expect(out).toHaveLength(2);
    expect(d.ensureModel).toHaveBeenCalledTimes(2);
  });

  it("document symbols come from the current buffer text", async () => {
    const d = deps({
      documentSymbols: vi.fn(async () => [
        { name: "A", kind: "class", line: 0, col: 0, endLine: 3, endCol: 1, selLine: 0, selCol: 6, children: [] },
      ]),
    });
    const p = buildProviders(monaco, d);
    const out = await p.documentSymbol.provideDocumentSymbols(model("orrery://a/x.ts", "foo", "class A {}"), live);
    expect(d.documentSymbols).toHaveBeenCalledWith("a", "x.ts", "class A {}");
    expect(out).toHaveLength(1);
    expect(out![0].kind).toBe(SymbolKind.Class);
  });
});
