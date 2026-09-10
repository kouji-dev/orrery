/**
 * Symbol-search helpers — pure logic tests, NO TestBed.
 *
 * Angular's vitest JIT does not apply the signal-input compiler transform
 * (NG0950 the moment a component with `input.required` renders); the repo
 * convention is therefore to export the pure helpers and test those in
 * isolation — see backlog/ticket-card.component.spec.ts:1-9.
 */
import { describe, expect, it } from "vitest";
import { escapeLiteral, parseSymbol, symbolPattern } from "./symbol-search.service";

/** Exactly the set src-tauri/src/search/mod.rs escape_literal escapes. */
const SPECIALS = ["\\", ".", "+", "*", "?", "(", ")", "|", "[", "]", "{", "}", "^", "$", "#", "&", "-", "~"];

describe("escapeLiteral", () => {
  it("escapes every character of the backend's escape_literal set", () => {
    for (const c of SPECIALS) expect(escapeLiteral(c)).toBe("\\" + c);
  });

  it("leaves ordinary identifier characters untouched", () => {
    expect(escapeLiteral("handleFetch_2")).toBe("handleFetch_2");
    expect(escapeLiteral("")).toBe("");
  });

  it("escapes each special inside a longer query", () => {
    expect(escapeLiteral("a.b+c")).toBe("a\\.b\\+c");
    expect(escapeLiteral("on-$click")).toBe("on\\-\\$click");
  });

  it("does not escape characters outside the set", () => {
    // ':' ',' '/' '<' '>' '!' '=' are NOT in escape_literal
    expect(escapeLiteral("a:b,c/d<e>f!g=h")).toBe("a:b,c/d<e>f!g=h");
  });
});

describe("symbolPattern", () => {
  it("embeds the ESCAPED query, not the raw one", () => {
    expect(symbolPattern("a.b")).toContain("a\\.b");
    expect(symbolPattern("a.b")).not.toContain("*a.b*");
  });

  it("keeps the declaration keywords and the identifier windows", () => {
    const p = symbolPattern("Fetch");
    expect(p.startsWith("\\b(?:")).toBe(true);
    for (const k of ["fn", "class", "interface", "struct", "def", "const"]) expect(p).toContain(k);
    expect(p).toContain("\\s+[A-Za-z0-9_$]*Fetch[A-Za-z0-9_$]*");
  });

  it("produces a regex that matches prefix, infix and suffix queries", () => {
    const line = "pub fn handleFetch(&self) {";
    for (const q of ["handle", "dleFe", "Fetch"]) {
      expect(new RegExp(symbolPattern(q)).test(line)).toBe(true);
    }
    expect(new RegExp(symbolPattern("zzz")).test(line)).toBe(false);
  });

  it("matches the full keyword+identifier span (what ranges[0] carries)", () => {
    const m = new RegExp(symbolPattern("handle")).exec("pub fn handleFetch(&self) {");
    expect(m?.[0]).toBe("fn handleFetch");
  });
});

describe("parseSymbol", () => {
  it("reads the declaring keyword past a visibility prefix", () => {
    expect(parseSymbol("pub fn handleFetch")).toEqual({ kind: "fn", name: "handleFetch" });
  });

  it("reads an exported class", () => {
    expect(parseSymbol("export class FetchStore")).toEqual({ kind: "class", name: "FetchStore" });
  });

  it("tolerates leading whitespace (indented python)", () => {
    expect(parseSymbol("  def handle_fetch")).toEqual({ kind: "def", name: "handle_fetch" });
  });

  it("handles the other declaration keywords", () => {
    expect(parseSymbol("type Fetch")).toEqual({ kind: "type", name: "Fetch" });
    expect(parseSymbol("const fetchAll")).toEqual({ kind: "const", name: "fetchAll" });
    expect(parseSymbol("impl $Weird_1")).toEqual({ kind: "impl", name: "$Weird_1" });
  });

  it("returns null for junk", () => {
    expect(parseSymbol("")).toBeNull();
    expect(parseSymbol("handleFetch")).toBeNull();
    expect(parseSymbol("return handleFetch")).toBeNull();
    expect(parseSymbol("fn 9lives")).toBeNull();
  });
});
