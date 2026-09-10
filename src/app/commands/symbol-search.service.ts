import { DestroyRef, inject, Injectable, signal } from "@angular/core";
import {
  BRIDGE,
  Commands,
  Events,
  SearchDonePayload,
  SearchMatchEntry,
  SearchResultsPayload,
} from "../data-source/bridge";
import { ScopeKind } from "../shared/scope";

/** One symbol declaration found by the grep pass. */
export interface SymbolHit {
  /** Stable render key (agent + path + line + name). */
  key: string;
  /** The declared identifier — what the fuzzy ranking scores against. */
  name: string;
  /** The declaring keyword (fn / class / type / …) — shown as the row badge. */
  kind: string;
  /** Root-relative path, forward-slash separated. */
  path: string;
  line: number;
  /** Worktree the hit came from (null = the project checkout). */
  agentId: string | null;
  /** Human label of that root (agent name); null = project checkout. */
  root: string | null;
}

/**
 * Declaration keywords we look for. Deliberately cross-language and flat: the
 * app has no tree-sitter layer, so "symbols" here means "a line that declares
 * something", which a single regex can find in every language at once.
 */
const KEYWORDS = [
  "fn",
  "func",
  "function",
  "class",
  "interface",
  "type",
  "struct",
  "enum",
  "trait",
  "impl",
  "def",
  "module",
  "namespace",
  "const",
  "let",
  "var",
  "val",
] as const;

/** EXACTLY the set `escape_literal` escapes in src-tauri/src/search/mod.rs
 *  (:138-151). Kept in lockstep on purpose: the query is spliced into a
 *  `regex: true` pattern, so anything the backend would have escaped for a
 *  literal search has to be escaped here instead. */
const ESCAPED = new Set([
  "\\",
  ".",
  "+",
  "*",
  "?",
  "(",
  ")",
  "|",
  "[",
  "]",
  "{",
  "}",
  "^",
  "$",
  "#",
  "&",
  "-",
  "~",
]);

/** Escape a user query so it can be spliced into a regex pattern verbatim. */
export function escapeLiteral(q: string): string {
  let out = "";
  for (const c of q) out += (ESCAPED.has(c) ? "\\" : "") + c;
  return out;
}

/**
 * The declaration pattern for `q`.
 *
 * Why the identifier prefix is `[A-Za-z0-9_$]*` (zero-or-more) rather than
 * `[A-Za-z_$][A-Za-z0-9_$]*`: `\s+` already anchors the identifier to the
 * character right after the keyword, so a required leading char would make
 * PREFIX queries — the common case, `fn handleFetch` typed as "handle" — fail
 * to match at all. Zero-or-more keeps prefix, infix and suffix queries working.
 */
export function symbolPattern(q: string): string {
  return `\\b(?:${KEYWORDS.join("|")})\\s+[A-Za-z0-9_$]*${escapeLiteral(q)}[A-Za-z0-9_$]*`;
}

/** The keyword + identifier tail of a matched span. Anchored at the END so a
 *  span that carries a visibility prefix ("pub fn handleFetch") still resolves
 *  to the declaring keyword and not to the prefix. */
const SYMBOL_RE = new RegExp(`(?:^|\\b)(${KEYWORDS.join("|")})\\s+([A-Za-z_$][\\w$]*)\\s*$`);

/** `"pub fn handleFetch"` → `{ kind: "fn", name: "handleFetch" }`; null = junk. */
export function parseSymbol(span: string): { kind: string; name: string } | null {
  const m = SYMBOL_RE.exec(span);
  return m ? { kind: m[1], name: m[2] } : null;
}

/**
 * Symbol lookup on top of the existing streaming grep engine (src-tauri/src/
 * search) — no backend change: `regex: true` requests pass the pattern through
 * VERBATIM, so a declaration regex is just another search.
 *
 * Root-provided because the Search-Everywhere overlay is destroyed on every
 * close; a per-instance service would drop an in-flight search's events on the
 * floor instead of cancelling it.
 */
@Injectable({ providedIn: "root" })
export class SymbolSearchService {
  private bridge = inject(BRIDGE);

  readonly hits = signal<SymbolHit[]>([]);
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly truncated = signal(false);

  private searchId: string | null = null;
  /** Batches that raced ahead of the invoke's searchId resolution. Only filled
   *  while a start is PENDING — `search://results` is one global channel shared
   *  with Find in Files, so buffering unconditionally would swallow that
   *  panel's batches into a queue we later replay as our own. */
  private starting = false;
  private earlyBatches: SearchResultsPayload[] = [];
  /** Bumped per run so a slow invoke from an abandoned run cannot install its
   *  id over the newer one. */
  private gen = 0;
  private unsubs: (() => void)[] = [];

  constructor() {
    void this.bridge
      .on<SearchResultsPayload>(Events.SearchResults, (p) => this.onResults(p))
      .then((u) => this.unsubs.push(u));
    void this.bridge
      .on<SearchDonePayload>(Events.SearchDone, (p) => this.onDone(p))
      .then((u) => this.unsubs.push(u));
    inject(DestroyRef).onDestroy(() => {
      this.unsubs.forEach((u) => u());
      this.cancel();
    });
  }

  /** Start a symbol search. Fire-and-forget: results land in `hits()`. */
  search(q: string, opts: { kind: ScopeKind; agentId: string | null; projectId: string | null }): void {
    void this.run(q, opts);
  }

  /** Stop the in-flight search, keeping whatever streamed in so far (the Stop
   *  button reads as "stop growing", not "throw away"). */
  cancel(): void {
    this.starting = false;
    this.earlyBatches = [];
    this.gen++;
    if (this.searchId) {
      const id = this.searchId;
      this.searchId = null;
      void this.bridge.invoke(Commands.SearchCancel, { id }).catch(() => {});
    }
    this.busy.set(false);
  }

  private async run(
    q: string,
    opts: { kind: ScopeKind; agentId: string | null; projectId: string | null },
  ): Promise<void> {
    this.cancel();
    const gen = this.gen;
    this.hits.set([]);
    this.truncated.set(false);
    this.error.set(null);
    // Why 2 chars minimum + maxResults 300: DEFAULT_MAX_RESULTS is 2000 with
    // MAX_PER_FILE 200 (src-tauri/src/search/mod.rs:33-36), so a 1-char query
    // matches nearly every declaration in the tree and truncates before it ever
    // reaches anything the user meant. A tight cap keeps the stream short too.
    if (q.trim().length < 2) return;
    if (opts.kind === "worktree" && !opts.agentId) {
      this.error.set("open a worktree to search its symbols");
      return;
    }
    if (opts.kind !== "worktree" && !opts.projectId) {
      this.error.set("no project to search");
      return;
    }
    this.busy.set(true);
    this.starting = true;
    try {
      const id = await this.bridge.invoke<string>(Commands.SearchStart, {
        req: {
          query: symbolPattern(q.trim()),
          caseSensitive: false,
          wholeWord: false,
          regex: true,
          scope: opts.kind,
          agentId: opts.agentId,
          projectId: opts.projectId,
          maxResults: 300,
          replacement: null,
        },
      });
      if (gen !== this.gen) {
        // a newer run started while this invoke was in flight — drop this one
        void this.bridge.invoke(Commands.SearchCancel, { id }).catch(() => {});
        return;
      }
      this.searchId = id;
      this.starting = false;
      const early = this.earlyBatches;
      this.earlyBatches = [];
      early.forEach((p) => this.onResults(p));
    } catch (e) {
      if (gen !== this.gen) return;
      this.starting = false;
      this.busy.set(false);
      this.error.set((e as { message?: string })?.message ?? "symbol search failed");
    }
  }

  private onResults(p: SearchResultsPayload): void {
    if (!this.searchId) {
      if (this.starting) this.earlyBatches.push(p);
      return;
    }
    if (p.searchId !== this.searchId) return; // Find in Files shares this channel
    const add = p.items.map((m) => this.toHit(m)).filter((h): h is SymbolHit => !!h);
    if (add.length) this.hits.update((h) => [...h, ...add]);
  }

  private onDone(p: SearchDonePayload): void {
    if (p.searchId !== this.searchId) return;
    this.busy.set(false);
    this.truncated.set(p.truncated);
    this.searchId = null;
  }

  /** `ranges[0]` is the FULL match span (keyword + identifier) already remapped
   *  by `window_line` into UTF-16 units of `text`, so plain JS slicing lands on
   *  exactly the declaration — no re-parsing of the whole line. */
  private toHit(m: SearchMatchEntry): SymbolHit | null {
    // a clipped long-line window can drop every range while still emitting the
    // entry (mod.rs window_line) — nothing to extract, so skip it
    if (!m.ranges.length) return null;
    const [s, e] = m.ranges[0];
    const sym = parseSymbol(m.text.slice(s, e));
    if (!sym) return null;
    return {
      key: (m.agentId ?? "") + "|" + m.path + ":" + m.line + ":" + sym.name,
      name: sym.name,
      kind: sym.kind,
      path: m.path,
      line: m.line,
      agentId: m.agentId ?? null,
      root: m.root ?? null,
    };
  }
}
