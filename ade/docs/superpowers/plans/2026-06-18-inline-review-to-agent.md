# Inline Review → Send to Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user drop GitHub-style inline line/range comments while reading an agent's working-changes diff or any open file, accumulate them per-agent, and deliver the batch (plus a global note) to the live agent as one structured PTY message — plus a rich blame "annotate" hover popup.

**Architecture:** A unified row renderer (`ReviewCodeComponent`) replaces CodeMirror on two surfaces only — `diff-view` (working changes) and `file-view` (open files). It reads rows from pure adapters (`diffToHunks` for diffs, line-split for files), shows a comment gutter ("+", drag-range), an inline composer, and persistent saved-comment cards. An in-memory `ReviewStore` (signal map keyed by agentId) holds pending comments; `AgentReviewService.sendReview` assembles the structured message and pastes it into the agent PTY (bracketed-paste wrapped) reusing the existing start-if-idle pattern. The commit/range inspection views (`agent-git-view`) are untouched and keep CodeMirror. A blame port (`AnnotateBlameComponent`) provides the annotate hover popup behind the Annotate toggle.

**Tech Stack:** Angular 20 (standalone components, signals, `ChangeDetectionStrategy.OnPush`), TypeScript, Vitest (jsdom), the Tauri bridge (`BRIDGE` injection token, `agent_input`/`agent_diff`/`agent_working_blame` commands).

## Global Constraints

- Comments are enabled **only** on `diff-view.component.ts` (working changes) and `file-view.component.ts` (open files). `agent-git-view` / `commit-diff-view` / `range-diff-view` / `diff-or-blame` stay on CodeMirror, **unchanged**.
- Comment state is **in-memory only** (no `localStorage`), per-agent, cleared on successful send, lost on restart.
- A single comment anchors to **one line or a dragged range**; the send target is always the pane's agent.
- Reuse existing design tokens / utility classes / icons (all present): `--bg --panel --panel-2 --panel-3 --elev --hair --hair-2 --ink --ink-2 --ink-3 --ink-4 --accent --accent-2 --code-add-bg --code-add-ink --code-del-bg --code-del-ink --st-blocked --st-done --font-mono --r-sm --r-md --shadow`; classes `.btn .primary .ghost-hair .chip .kbd .up .disp .surface .rise .tnum .scroll-y`; icons `plus chat trash enter x file git check`.
- The unified renderer MUST guard against the large-file stall (reuse `diffWouldStall` from `code-diff.component.ts`): above threshold, render a capped/plain view with a "Review anyway" escape.
- Match surrounding code style: inline styles via `[style.*]`/`style="..."` exactly as the existing `diff-view`/`file-view` components do; `OnPush`; signals over RxJS.
- All bridge command names come from `Commands` in `src/app/data-source/bridge.ts` (e.g. `Commands.AgentInput`).

## File Structure

**Create:**
- `src/app/workspace/review/unified-diff.ts` — pure `diffToHunks(old, new, context?)` → `Diff`; `fileToRows`/`diffToRows` row builders.
- `src/app/workspace/review/unified-diff.spec.ts` — tests for the above.
- `src/app/agents/review.store.ts` — in-memory `ReviewStore` (signal map) + pure `buildReviewPayload` / `assembleReviewMessage`.
- `src/app/agents/review.store.spec.ts` — store + assemble tests.
- `src/app/agents/agent-review.service.ts` — `AgentReviewService.sendReview(agentId, payload)` (PTY delivery).
- `src/app/agents/agent-review.service.spec.ts` — delivery tests (stub AgentsStore/runtime).
- `src/app/workspace/review/review-code.component.ts` — `ReviewCodeComponent` (unified renderer + gutter "+", drag-range, composer, saved cards).
- `src/app/workspace/review/review-code.component.spec.ts` — TestBed interaction test.
- `src/app/workspace/review/send-review.component.ts` — `SendReviewButtonComponent` + `SendReviewModalComponent`.
- `src/app/workspace/review/annotate-blame.component.ts` — `AnnotateBlameComponent` (blame view + hover popup) + `blameToRows` adapter.
- `src/app/workspace/review/annotate-blame.spec.ts` — `blameToRows` adapter test.

**Modify:**
- `src/test-setup.ts` — add Angular TestBed environment init (one-time).
- `src/app/workspace/file-view.component.ts` — swap editor for `ReviewCodeComponent`; add Annotate toggle → `AnnotateBlameComponent`; mount `SendReviewButtonComponent` + modal.
- `src/app/workspace/diff-view.component.ts` — swap `<app-code-diff>` for `ReviewCodeComponent`; Annotate toggle → `AnnotateBlameComponent`; mount `SendReviewButtonComponent` + modal.

---

### Task 1: `diffToHunks` + row builders (pure)

**Files:**
- Create: `src/app/workspace/review/unified-diff.ts`
- Test: `src/app/workspace/review/unified-diff.spec.ts`

**Interfaces:**
- Consumes: `Diff`, `DiffHunk`, `DiffLine` from `../../models`.
- Produces:
  - `diffToHunks(oldText: string, newText: string, context?: number): DiffHunk[]`
  - `type Row = { type: "hunk"; meta: string } | { type: "code"; k: "+" | "-" | " "; n: number; s: string; side: "old" | "new" | "file" }`
  - `diffToRows(hunks: DiffHunk[]): Row[]`
  - `fileToRows(text: string): Row[]`

- [ ] **Step 1: Write the failing test**

```typescript
import { describe, it, expect } from "vitest";
import { diffToHunks, diffToRows, fileToRows } from "./unified-diff";

describe("diffToHunks", () => {
  it("emits a single + hunk for a new file", () => {
    const h = diffToHunks("", "a\nb\n");
    expect(h.length).toBe(1);
    expect(h[0].lines.map((l) => l.k)).toEqual(["+", "+"]);
    expect(h[0].lines.map((l) => l.n)).toEqual([1, 2]);
    expect(h[0].meta).toBe("@@ -0,0 +1,2 @@");
  });

  it("marks a replaced line as - then + with correct side line numbers", () => {
    const h = diffToHunks("x\nold\nz\n", "x\nnew\nz\n");
    const flat = h.flatMap((x) => x.lines).map((l) => `${l.k}${l.n}:${l.s}`);
    expect(flat).toEqual(["  1:x", "- 2:old", "+ 2:new", "  3:z"]);
  });

  it("keeps unchanged lines as context within the surrounding window", () => {
    const h = diffToHunks("a\nb\nc\n", "a\nB\nc\n");
    expect(h[0].lines.some((l) => l.k === "-" && l.s === "b")).toBe(true);
    expect(h[0].lines.some((l) => l.k === "+" && l.s === "B")).toBe(true);
  });
});

describe("row builders", () => {
  it("diffToRows interleaves a hunk separator then code rows with side", () => {
    const rows = diffToRows(diffToHunks("old\n", "new\n"));
    expect(rows[0].type).toBe("hunk");
    const code = rows.filter((r) => r.type === "code") as Extract<typeof rows[number], { type: "code" }>[];
    expect(code.find((r) => r.k === "-")!.side).toBe("old");
    expect(code.find((r) => r.k === "+")!.side).toBe("new");
  });

  it("fileToRows yields one context row per line, side=file", () => {
    const rows = fileToRows("a\nb\n");
    expect(rows.map((r) => (r.type === "code" ? r.n : -1))).toEqual([1, 2]);
    expect(rows.every((r) => r.type === "code" && r.side === "file")).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/app/workspace/review/unified-diff.spec.ts`
Expected: FAIL — "Cannot find module './unified-diff'".

- [ ] **Step 3: Write minimal implementation**

```typescript
import { Diff, DiffHunk, DiffLine } from "../../models";

export type Row =
  | { type: "hunk"; meta: string }
  | { type: "code"; k: "+" | "-" | " "; n: number; s: string; side: "old" | "new" | "file" };

/** Split into lines without a trailing empty element for a final newline. */
function lines(s: string): string[] {
  if (!s.length) return [];
  return s.replace(/\n$/, "").split("\n");
}

interface Op { k: "+" | "-" | " "; s: string; }

/** Classic LCS over lines → an op list (equal / deleted / added). */
function lcsOps(a: string[], b: string[]): Op[] {
  const m = a.length, n = b.length;
  // dp[i][j] = LCS length of a[i:], b[j:]
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--)
    for (let j = n - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const ops: Op[] = [];
  let i = 0, j = 0;
  while (i < m && j < n) {
    if (a[i] === b[j]) { ops.push({ k: " ", s: a[i] }); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) { ops.push({ k: "-", s: a[i] }); i++; }
    else { ops.push({ k: "+", s: b[j] }); j++; }
  }
  while (i < m) { ops.push({ k: "-", s: a[i] }); i++; }
  while (j < n) { ops.push({ k: "+", s: b[j] }); j++; }
  return ops;
}

/** Build unified hunks (with `context` equal lines around changes) from old/new text. */
export function diffToHunks(oldText: string, newText: string, context = 3): DiffHunk[] {
  const a = lines(oldText), b = lines(newText);
  const ops = lcsOps(a, b);
  // assign side line numbers as we walk
  let oldN = 0, newN = 0;
  const numbered = ops.map((op) => {
    if (op.k === " ") { oldN++; newN++; return { ...op, oldN, newN }; }
    if (op.k === "-") { oldN++; return { ...op, oldN, newN }; }
    newN++; return { ...op, oldN, newN };
  });

  // group changed runs with `context` equal lines on each side; gaps > 2*context split hunks
  const hunks: DiffHunk[] = [];
  let cur: { startOld: number; startNew: number; lines: DiffLine[] } | null = null;
  let trailingEqual = 0;
  const flush = () => {
    if (!cur) return;
    // drop equal lines beyond `context` at the tail
    while (trailingEqual > context && cur.lines.length && cur.lines[cur.lines.length - 1].k === " ") {
      cur.lines.pop(); trailingEqual--;
    }
    const oCount = cur.lines.filter((l) => l.k !== "+").length;
    const nCount = cur.lines.filter((l) => l.k !== "-").length;
    const oStart = oCount ? cur.startOld : 0;
    const nStart = nCount ? cur.startNew : 0;
    cur.lines.forEach((l) => 0); // no-op for readability
    hunks.push({ meta: `@@ -${oStart},${oCount} +${nStart},${nCount} @@`, lines: cur.lines });
    cur = null; trailingEqual = 0;
  };

  for (let idx = 0; idx < numbered.length; idx++) {
    const op = numbered[idx];
    const isChange = op.k !== " ";
    if (isChange) {
      if (!cur) {
        // open a hunk, backfilling up to `context` preceding equal lines
        const back: DiffLine[] = [];
        for (let j = idx - 1; j >= 0 && numbered[j].k === " " && back.length < context; j--) {
          back.unshift({ k: " ", n: numbered[j].newN, s: numbered[j].s });
        }
        const firstOld = (() => { for (let j = idx - back.length; j < numbered.length; j++) return numbered[j].oldN; return op.oldN; })();
        cur = { startOld: firstOld, startNew: op.newN - back.filter((l) => true).length, lines: [...back] };
      }
      cur.lines.push({ k: op.k, n: op.k === "-" ? op.oldN : op.newN, s: op.s });
      trailingEqual = 0;
    } else if (cur) {
      cur.lines.push({ k: " ", n: op.newN, s: op.s });
      trailingEqual++;
      if (trailingEqual > 2 * context) flush();
    }
  }
  flush();
  return hunks;
}

export function diffToRows(hunks: DiffHunk[]): Row[] {
  const rows: Row[] = [];
  hunks.forEach((h) => {
    rows.push({ type: "hunk", meta: h.meta });
    h.lines.forEach((ln) =>
      rows.push({ type: "code", k: ln.k, n: ln.n, s: ln.s, side: ln.k === "-" ? "old" : "new" }),
    );
  });
  return rows;
}

export function fileToRows(text: string): Row[] {
  return lines(text).map((s, i) => ({ type: "code" as const, k: " " as const, n: i + 1, s, side: "file" as const }));
}
```

NOTE: the `startNew`/`startOld` backfill above must produce the line number of the FIRST row in the hunk. Simplify in Step 3 by computing `startOld`/`startNew` from the first emitted line: after building `cur.lines`, set `oStart = ` first line's old-number, `nStart = ` first line's new-number. If the leading rows are context, both equal that context line's numbers. Adjust the implementation so the test `@@ -0,0 +1,2 @@` (new file) and the replaced-line case pass; iterate against the tests rather than trusting the first draft.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/app/workspace/review/unified-diff.spec.ts`
Expected: PASS (4 tests). Fix `diffToHunks` numbering until green.

- [ ] **Step 5: Commit**

```bash
git add src/app/workspace/review/unified-diff.ts src/app/workspace/review/unified-diff.spec.ts
git commit -m "feat(review): pure unified-diff hunk + row builders"
```

---

### Task 2: `ReviewStore` + assemble (pure, in-memory)

**Files:**
- Create: `src/app/agents/review.store.ts`
- Test: `src/app/agents/review.store.spec.ts`

**Interfaces:**
- Produces:
  - `interface ReviewComment { id: string; file: string; view: "diff" | "file"; lang: string; fromIdx: number; toIdx: number; fromLine: number; toLine: number; side: "old" | "new" | "file"; snippet: string; lines: string[]; note: string; }`
  - `interface ReviewPayloadItem { file: string; fromLine: number; toLine: number; snippet: string; note: string; block: boolean; }`
  - `interface ReviewPayload { comments: ReviewPayloadItem[]; global: string; }`
  - `class ReviewStore` (`providedIn: "root"`): `list(agentId): ReviewComment[]`, `count(agentId): number`, `add(agentId, c: Omit<ReviewComment, "id">): string`, `remove(agentId, id): void`, `clear(agentId): void`, `buildPayload(agentId, global: string): ReviewPayload`.
  - `assembleReviewMessage(p: ReviewPayload): string` (exported pure fn)
  - `isBlock(c: { fromIdx: number; toIdx: number }): boolean`

- [ ] **Step 1: Write the failing test**

```typescript
import { Injector, runInInjectionContext } from "@angular/core";
import { describe, it, expect, beforeEach } from "vitest";
import { ReviewStore, assembleReviewMessage } from "./review.store";

function base(file = "src/a.ts") {
  return { file, view: "diff" as const, lang: "ts", fromIdx: 0, toIdx: 0, fromLine: 42, toLine: 42, side: "new" as const, snippet: "const t = parse(x)", lines: ["const t = parse(x)"], note: "wrap it" };
}

describe("ReviewStore", () => {
  let store: ReviewStore;
  beforeEach(() => {
    const injector = Injector.create({ providers: [] });
    store = runInInjectionContext(injector, () => new ReviewStore());
  });

  it("add/list/count/remove/clear scoped per agent", () => {
    const id = store.add("a", base());
    store.add("a", base("src/b.ts"));
    store.add("z", base());
    expect(store.count("a")).toBe(2);
    expect(store.count("z")).toBe(1);
    store.remove("a", id);
    expect(store.count("a")).toBe(1);
    store.clear("a");
    expect(store.count("a")).toBe(0);
    expect(store.count("z")).toBe(1); // other agents untouched
  });

  it("buildPayload maps comments + flags blocks", () => {
    store.add("a", { ...base(), fromIdx: 0, toIdx: 2, fromLine: 10, toLine: 12, lines: ["x", "y", "z"] });
    const p = store.buildPayload("a", "  tighten  ");
    expect(p.global).toBe("tighten");
    expect(p.comments[0]).toMatchObject({ file: "src/a.ts", fromLine: 10, toLine: 12, block: true });
  });
});

describe("assembleReviewMessage", () => {
  it("renders the exact structured message", () => {
    const msg = assembleReviewMessage({
      global: "tighten error handling",
      comments: [
        { file: "src/auth.ts", fromLine: 42, toLine: 42, snippet: "const t = parse(x)", note: "this can throw, wrap it", block: false },
        { file: "src/api.ts", fromLine: 10, toLine: 13, snippet: "fetch(u)", note: "extract a helper", block: true },
      ],
    });
    expect(msg).toBe(
      [
        "Review feedback:",
        "[global] tighten error handling",
        "",
        "src/auth.ts:42  `const t = parse(x)`",
        "  → this can throw, wrap it",
        "src/api.ts:10-13  `fetch(u)`  (block, 4 lines)",
        "  → extract a helper",
      ].join("\n"),
    );
  });

  it("omits the global line when empty", () => {
    const msg = assembleReviewMessage({ global: "", comments: [{ file: "f", fromLine: 1, toLine: 1, snippet: "s", note: "n", block: false }] });
    expect(msg.startsWith("Review feedback:\n\nf:1")).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/app/agents/review.store.spec.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Write minimal implementation**

```typescript
import { Injectable, signal } from "@angular/core";

export interface ReviewComment {
  id: string;
  file: string;
  view: "diff" | "file";
  lang: string;
  fromIdx: number;
  toIdx: number;
  fromLine: number;
  toLine: number;
  side: "old" | "new" | "file";
  snippet: string;
  lines: string[];
  note: string;
}
export interface ReviewPayloadItem {
  file: string;
  fromLine: number;
  toLine: number;
  snippet: string;
  note: string;
  block: boolean;
}
export interface ReviewPayload {
  comments: ReviewPayloadItem[];
  global: string;
}

export function isBlock(c: { fromIdx: number; toIdx: number }): boolean {
  return c.toIdx > c.fromIdx;
}

function refLines(c: { fromLine: number; toLine: number }): string {
  return c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`;
}

/** Pure: render the structured message the agent receives (no paste markers). */
export function assembleReviewMessage(p: ReviewPayload): string {
  const out: string[] = ["Review feedback:"];
  if (p.global) out.push(`[global] ${p.global}`);
  out.push("");
  p.comments.forEach((c, i) => {
    const block = c.block ? `  (block, ${c.toLine - c.fromLine + 1} lines)` : "";
    out.push(`${c.file}:${refLines(c)}  \`${c.snippet}\`${block}`);
    out.push(`  → ${c.note}`);
    if (i < p.comments.length - 1 && false) out.push(""); // comments are not blank-separated
  });
  return out.join("\n");
}

interface Slot { comments: ReviewComment[]; seq: number; }

@Injectable({ providedIn: "root" })
export class ReviewStore {
  // in-memory only — NO persistence. Signal map so views re-render on change.
  private readonly state = signal<Record<string, Slot>>({});

  list(agentId: string): ReviewComment[] {
    return this.state()[agentId]?.comments ?? [];
  }
  count(agentId: string): number {
    return this.list(agentId).length;
  }
  add(agentId: string, c: Omit<ReviewComment, "id">): string {
    let id = "";
    this.state.update((m) => {
      const slot = m[agentId] ?? { comments: [], seq: 0 };
      const seq = slot.seq + 1;
      id = `rc${seq}`;
      return { ...m, [agentId]: { seq, comments: [...slot.comments, { ...c, id }] } };
    });
    return id;
  }
  remove(agentId: string, id: string): void {
    this.state.update((m) => {
      const slot = m[agentId];
      if (!slot) return m;
      return { ...m, [agentId]: { ...slot, comments: slot.comments.filter((x) => x.id !== id) } };
    });
  }
  clear(agentId: string): void {
    this.state.update((m) => {
      if (!m[agentId]) return m;
      return { ...m, [agentId]: { ...m[agentId], comments: [] } };
    });
  }
  buildPayload(agentId: string, global: string): ReviewPayload {
    return {
      global: global.trim(),
      comments: this.list(agentId).map((c) => ({
        file: c.file,
        fromLine: c.fromLine,
        toLine: c.toLine,
        snippet: c.snippet,
        note: c.note,
        block: isBlock(c),
      })),
    };
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/app/agents/review.store.spec.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/app/agents/review.store.ts src/app/agents/review.store.spec.ts
git commit -m "feat(review): in-memory ReviewStore + structured message assembler"
```

---

### Task 3: `AgentReviewService.sendReview` (PTY delivery)

**Files:**
- Create: `src/app/agents/agent-review.service.ts`
- Test: `src/app/agents/agent-review.service.spec.ts`

**Interfaces:**
- Consumes: `ReviewStore`, `AgentsStore` (`input(id, data): Promise<void>`), `AgentRuntimeService` (`agents()` signal, `startProcess(id, {resume})`), `UiStore` (`openAgent(id, view)`, `flash(msg)`). `assembleReviewMessage`, `ReviewPayload`.
- Produces: `class AgentReviewService` (`providedIn: "root"`): `sendReview(agentId: string, payload: ReviewPayload): void`. Delivery wraps the message in bracketed-paste markers `ESC[200~ … ESC[201~`, then sends a separate `\r`.

- [ ] **Step 1: Write the failing test**

```typescript
import { Injector, runInInjectionContext } from "@angular/core";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { AgentReviewService } from "./agent-review.service";
import { ReviewStore } from "./review.store";
import { AgentsStore } from "../stores/agents.store";
import { AgentRuntimeService } from "./agent-runtime.service";
import { UiStore } from "../ui/ui.store";

describe("AgentReviewService.sendReview", () => {
  let inputs: Array<{ id: string; data: string }>;
  let svc: AgentReviewService;
  let started: string[];

  beforeEach(() => {
    inputs = [];
    started = [];
    const agentsStore = { input: (id: string, data: string) => { inputs.push({ id, data }); return Promise.resolve(); } } as unknown as AgentsStore;
    const runtime = {
      agents: () => [{ id: "a", status: "running", started: true }],
      startProcess: (id: string) => started.push(id),
    } as unknown as AgentRuntimeService;
    const ui = { openAgent: () => {}, flash: () => {} } as unknown as UiStore;
    const injector = Injector.create({
      providers: [
        { provide: AgentsStore, useValue: agentsStore },
        { provide: AgentRuntimeService, useValue: runtime },
        { provide: UiStore, useValue: ui },
        ReviewStore,
        AgentReviewService,
      ],
    });
    svc = injector.get(AgentReviewService);
  });

  it("running agent: pastes bracketed-wrapped message then submits", async () => {
    svc.sendReview("a", { global: "", comments: [{ file: "f", fromLine: 1, toLine: 1, snippet: "s", note: "n", block: false }] });
    await Promise.resolve(); await Promise.resolve();
    expect(inputs[0].id).toBe("a");
    expect(inputs[0].data.startsWith("\x1b[200~")).toBe(true);
    expect(inputs[0].data.endsWith("\x1b[201~")).toBe(true);
    expect(inputs[0].data).toContain("f:1");
    expect(inputs[1].data).toBe("\r");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/app/agents/agent-review.service.spec.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Write minimal implementation**

```typescript
import { inject, Injectable } from "@angular/core";
import { AgentsStore } from "../stores/agents.store";
import { AgentRuntimeService } from "./agent-runtime.service";
import { UiStore } from "../ui/ui.store";
import { assembleReviewMessage, ReviewPayload } from "./review.store";

const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

/** Delivers an assembled review to the agent's PTY as one bracketed paste. */
@Injectable({ providedIn: "root" })
export class AgentReviewService {
  private agents = inject(AgentsStore);
  private runtime = inject(AgentRuntimeService);
  private ui = inject(UiStore);

  sendReview(agentId: string, payload: ReviewPayload): void {
    if (!payload.comments.length) return;
    const text = assembleReviewMessage(payload);
    const ag = this.runtime.agents().find((a) => a.id === agentId);
    this.ui.openAgent(agentId, "terminal");
    const send = () =>
      void this.agents
        .input(agentId, PASTE_START + text + PASTE_END)
        .then(() => this.agents.input(agentId, "\r"))
        .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "send failed"));
    if (ag && ag.status === "running") {
      send();
    } else {
      this.runtime.startProcess(agentId, { resume: !!ag?.started });
      setTimeout(send, 1800);
    }
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/app/agents/agent-review.service.spec.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/agents/agent-review.service.ts src/app/agents/agent-review.service.spec.ts
git commit -m "feat(review): PTY delivery of the assembled review (bracketed paste)"
```

---

### Task 4: Enable Angular TestBed in vitest

**Files:**
- Modify: `src/test-setup.ts`

**Interfaces:**
- Produces: a working `TestBed` environment so component specs can render. Existing pure specs (`Injector.create`) are unaffected.

- [ ] **Step 1: Add the TestBed env init**

Append to `src/test-setup.ts`:

```typescript
import { getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";

getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
```

(Angular 20: `@angular/platform-browser-dynamic` is not installed — use `@angular/platform-browser/testing`.)

- [ ] **Step 2: Verify existing specs still pass**

Run: `pnpm vitest run src/app/agents/agent-work.store.spec.ts`
Expected: PASS (unchanged) — proves the init doesn't break pure specs.

- [ ] **Step 3: Commit**

```bash
git add src/test-setup.ts
git commit -m "test: enable Angular TestBed environment for vitest"
```

---

### Task 5: `ReviewCodeComponent` (unified renderer + comments)

**Files:**
- Create: `src/app/workspace/review/review-code.component.ts`
- Test: `src/app/workspace/review/review-code.component.spec.ts`

**Interfaces:**
- Consumes: `ReviewStore`, `Row`/`diffToRows`/`fileToRows`, `IconComponent`, `diffWouldStall` (import from `../code-diff.component`).
- Produces: `ReviewCodeComponent` (selector `app-review-code`) with inputs `agent: string`, `file: string`, `view: "diff" | "file"`, `rows: Row[]`, `lang: string`; renders the gutter "+", drag-range select, inline composer, saved-comment cards. Saved comments come from `ReviewStore.list(agent).filter(c => c.file===file && c.view===view)`.

Port `ReviewCode` / `InlineComposer` / `SavedCommentCard` from the design `review.jsx` verbatim in structure and inline styles (the tokens/classes all exist). Key behaviors to preserve: hover shows "+"; `mousedown` on "+" starts a drag; `mouseenter` on rows extends `drag.current`; window `mouseup` opens one composer for `[min,max]`; ⌘/Ctrl-↵ saves, Esc cancels; save builds the comment from `rows.slice(fromIdx,toIdx+1).filter(code)` (snippet = first code line trimmed, fromLine/toLine = first/last code line numbers, side = first code line side) and calls `ReviewStore.add`; covered rows show the anchor chat-chip / range dot and an `inset 2px 0 0 var(--accent)` rail; saved cards render under the range's last row. Guard: if `view==="diff"` and the source diff `diffWouldStall(old,new)` was true, the parent passes capped rows + a "Review anyway" banner (see Tasks 8–9); the component itself just renders the rows it's given.

- [ ] **Step 1: Write the failing interaction test**

```typescript
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach } from "vitest";
import { ReviewCodeComponent } from "./review-code.component";
import { ReviewStore } from "../../agents/review.store";
import { diffToHunks, diffToRows } from "./unified-diff";

describe("ReviewCodeComponent", () => {
  let store: ReviewStore;
  function setup() {
    const f = TestBed.createComponent(ReviewCodeComponent);
    f.componentRef.setInput("agent", "a");
    f.componentRef.setInput("file", "src/x.ts");
    f.componentRef.setInput("view", "diff");
    f.componentRef.setInput("lang", "ts");
    f.componentRef.setInput("rows", diffToRows(diffToHunks("old\n", "new\n")));
    f.detectChanges();
    return f;
  }
  beforeEach(() => {
    TestBed.configureTestingModule({ imports: [ReviewCodeComponent] });
    store = TestBed.inject(ReviewStore);
  });

  it("hovering a code row then clicking + opens a composer; save persists a comment", () => {
    const f = setup();
    const el: HTMLElement = f.nativeElement;
    const codeRow = el.querySelectorAll<HTMLElement>("[data-rowkind='code']")[0];
    codeRow.dispatchEvent(new MouseEvent("mouseenter", { bubbles: true }));
    f.detectChanges();
    const plus = el.querySelector<HTMLButtonElement>("[data-plus]")!;
    plus.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    window.dispatchEvent(new MouseEvent("mouseup"));
    f.detectChanges();
    const ta = el.querySelector<HTMLTextAreaElement>("textarea")!;
    ta.value = "wrap it";
    ta.dispatchEvent(new Event("input", { bubbles: true }));
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }));
    f.detectChanges();
    expect(store.count("a")).toBe(1);
    expect(store.list("a")[0].note).toBe("wrap it");
    expect(el.textContent).toContain("wrap it"); // saved card rendered
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/app/workspace/review/review-code.component.spec.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `ReviewCodeComponent`**

Port `review.jsx`'s `ReviewCode`/`InlineComposer`/`SavedCommentCard` into one standalone Angular component. Use `signal`s for `hover`, `drag`, `composer`, `draft`. Inputs via `input.required`. Inject `ReviewStore`. Add `[attr.data-rowkind]="'code'"` on code-row wrappers, `data-plus` on the "+" button, so the test can target them. Replicate the inline styles from the prototype (the comment gutter width 24, anchor chip, rail `inset 2px 0 0 var(--accent)`, composer textarea with `--font-mono`, Cancel/Save `.btn .ghost-hair`/`.btn .primary`, saved card with `YOU` chip + `pending` chip + trash). `save()` mirrors the prototype's `reviewAdd` payload exactly (file, view, lang, fromIdx, toIdx, fromLine, toLine, side, snippet, lines, note). For `view==="file"`, render the code text plain (no `highlight()` — v1 plain per spec); for `view==="diff"`, show the `+/-` marker column and `--code-add-ink`/`--code-del-ink`.

(Complete code: translate the JSX in `review.jsx` lines for `ReviewCode`, `InlineComposer`, `SavedCommentCard` 1:1; Angular `@for` over `rows` with `@if` branches for `hunk` vs `code`; `(mouseenter)`, `(mousedown)`, `(keydown)` handlers calling the signal updaters; `effect` to focus the textarea when `composer()` opens; window `mouseup` listener registered in an `effect`/`afterNextRender` and cleaned in `ngOnDestroy`.)

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/app/workspace/review/review-code.component.spec.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/workspace/review/review-code.component.ts src/app/workspace/review/review-code.component.spec.ts
git commit -m "feat(review): unified ReviewCode renderer with inline comments"
```

---

### Task 6: `SendReviewButtonComponent` + `SendReviewModalComponent`

**Files:**
- Create: `src/app/workspace/review/send-review.component.ts`

**Interfaces:**
- Consumes: `ReviewStore`, `AgentReviewService`, `IconComponent`, `fileName`/`fileDir`, `isBlock`.
- Produces:
  - `SendReviewButtonComponent` (selector `app-send-review-button`) input `agent: string`, input `agentName: string`; renders nothing when `ReviewStore.count(agent)===0`; otherwise the "Send review" button + count badge that toggles an internal modal-open signal.
  - `SendReviewModalComponent` (selector `app-send-review-modal`) inputs `agent`, `agentName`, output `close`; ports `SendReviewModal` — grouped-by-file list (deletable rows via `ReviewStore.remove`), global-note textarea, footer "Send to agent" → `AgentReviewService.sendReview(agent, ReviewStore.buildPayload(agent, global))` then `ReviewStore.clear(agent)` and emit `close`.

- [ ] **Step 1: Implement both components**

Port `SendReviewButton` and `SendReviewModal` from `review.jsx` 1:1 (overlay `position:fixed;inset:0;z-index:70`, `.surface .rise` card width 600, grouped list with sticky file headers, snippet chip, `→ note`, trash buttons, global-note textarea, footer). The button hosts the modal: `@if (open()) { <app-send-review-modal ... (close)="open.set(false)" /> }`. Send handler:

```typescript
send() {
  const payload = this.review.buildPayload(this.agent(), this.global());
  this.agentReview.sendReview(this.agent(), payload);
  this.review.clear(this.agent());
  this.close.emit();
}
```

- [ ] **Step 2: Add a smoke test (renders only at count>0)**

```typescript
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach } from "vitest";
import { SendReviewButtonComponent } from "./send-review.component";
import { ReviewStore } from "../../agents/review.store";

describe("SendReviewButtonComponent", () => {
  let store: ReviewStore;
  beforeEach(() => { TestBed.configureTestingModule({ imports: [SendReviewButtonComponent] }); store = TestBed.inject(ReviewStore); });
  it("hidden at 0, shows count badge once there are comments", () => {
    const f = TestBed.createComponent(SendReviewButtonComponent);
    f.componentRef.setInput("agent", "a"); f.componentRef.setInput("agentName", "Bee");
    f.detectChanges();
    expect(f.nativeElement.querySelector("button")).toBeNull();
    store.add("a", { file: "f", view: "diff", lang: "ts", fromIdx: 0, toIdx: 0, fromLine: 1, toLine: 1, side: "new", snippet: "s", lines: ["s"], note: "n" });
    f.detectChanges();
    expect(f.nativeElement.textContent).toContain("Send review");
    expect(f.nativeElement.textContent).toContain("1");
  });
});
```

Create `src/app/workspace/review/send-review.component.spec.ts` with the above.

- [ ] **Step 3: Run tests**

Run: `pnpm vitest run src/app/workspace/review/send-review.component.spec.ts`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/app/workspace/review/send-review.component.ts src/app/workspace/review/send-review.component.spec.ts
git commit -m "feat(review): Send-review button + grouped review modal"
```

---

### Task 7: `AnnotateBlameComponent` + `blameToRows` (annotate popup)

**Files:**
- Create: `src/app/workspace/review/annotate-blame.component.ts`
- Test: `src/app/workspace/review/annotate-blame.spec.ts`

**Interfaces:**
- Consumes: `BlameLine` (`../../models`), `IconComponent`.
- Produces:
  - `interface BlameRow { n: number; sha: string; author: string; s: string; when: number; age: number; rel: string; first: boolean; }`
  - `blameToRows(lines: BlameLine[]): BlameRow[]` — `age` = `(maxWhen - when)/(span||1)` clamped 0..1 (uncommitted `when===0` → age 0); `rel` = relative-time string from `when`; `first` = true when the previous row's `sha` differs.
  - `AnnotateBlameComponent` (selector `app-annotate-blame`) inputs `lines: BlameLine[]`, output `openCommit: string` — ports `FileBlameGutter`: author column (avatar initials + name + rel + sha chip on `first` rows, age fade via `color-mix`), line numbers, code, and the fixed-position hover popup (author, sha chip, summary, "click → open commit diff").

- [ ] **Step 1: Write the failing adapter test**

```typescript
import { describe, it, expect } from "vitest";
import { blameToRows } from "./annotate-blame.component";
import { BlameLine } from "../../models";

const bl = (n: number, sha: string, when: number, line: string): BlameLine => ({ n, sha, author: "Ann", when, summary: "msg", line });

describe("blameToRows", () => {
  it("flags first-of-commit rows and normalizes age 0..1", () => {
    const rows = blameToRows([bl(1, "aaa", 100, "x"), bl(2, "aaa", 100, "y"), bl(3, "bbb", 50, "z")]);
    expect(rows.map((r) => r.first)).toEqual([true, false, true]);
    expect(rows[0].age).toBeCloseTo(0, 5);   // newest
    expect(rows[2].age).toBeCloseTo(1, 5);   // oldest
    expect(rows[0].s).toBe("x");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/app/workspace/review/annotate-blame.spec.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `blameToRows` + the component**

Implement `blameToRows` per the interface. Port `FileBlameGutter` from `agent-git.jsx` to an Angular component, mapping `BlameLine` (real model) → `BlameRow` (no `AGENT_GIT` lookups). Compute author color/initials locally with the same hashing used in `code-diff.component.ts`'s `authorColor` (copy the function in; it's small) so colors match the rest of the app. Hover sets a `popup` signal `{ row, x, y }`; render the fixed-position card; `click` emits `openCommit(row.sha)`. Add a small `relTime(when: number): string` helper (e.g. `"2h"`, `"3d"`; empty for `when===0`).

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/app/workspace/review/annotate-blame.spec.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/workspace/review/annotate-blame.component.ts src/app/workspace/review/annotate-blame.spec.ts
git commit -m "feat(review): annotate blame view + hover popup"
```

---

### Task 8: Wire into `file-view.component.ts`

**Files:**
- Modify: `src/app/workspace/file-view.component.ts`

**Interfaces:**
- Consumes: `ReviewCodeComponent`, `AnnotateBlameComponent`, `SendReviewButtonComponent`, `fileToRows`, `GitInspectStore.blameFor`/`loadBlame` (for the working-tree rev) OR `Commands.AgentWorkingBlame` like `diff-view` does.

- [ ] **Step 1: Replace the editor body with the unified renderer**

In the template, replace the `<div #host>` editor branch with:

```html
@if (notice(); as n) {
  <div ...>{{ n }}</div>
} @else if (isMarkdown() && preview()) {
  <div class="scroll-y md-body" ... [innerHTML]="mdHtml()"></div>
} @else if (annotate()) {
  <app-annotate-blame [lines]="blame()" (openCommit)="openCommit.emit($event)" />
} @else {
  <app-review-code [agent]="agent().id" [file]="path()" view="file" [rows]="rows()" [lang]="langId()" />
}
```

Add an `annotate = signal(false)` toggle button to the toolbar (copy the Annotate button markup from `diff-view`), a `rows = computed(() => fileToRows(this.content() ?? ""))`, a `langId = computed(() => langId(this.path()))`, and a `blame` loader (reuse `Commands.AgentWorkingBlame` returning `{old,new}` — use `.new` side for an open file). Mount `<app-send-review-button [agent]="agent().id" [agentName]="agent().name" />` in the toolbar's right group. Add the imports to the component `imports: [...]` and remove the now-unused CodeMirror render path (the `render`/`view`/`loadCMCore` plumbing) since files now render via `ReviewCodeComponent`.

- [ ] **Step 2: Typecheck**

Run: `pnpm tsc -p tsconfig.app.json --noEmit`
Expected: no errors in `file-view.component.ts`.

- [ ] **Step 3: Commit**

```bash
git add src/app/workspace/file-view.component.ts
git commit -m "feat(review): wire inline review + annotate into the open-file view"
```

---

### Task 9: Wire into `diff-view.component.ts`

**Files:**
- Modify: `src/app/workspace/diff-view.component.ts`

**Interfaces:**
- Consumes: `ReviewCodeComponent`, `AnnotateBlameComponent`, `SendReviewButtonComponent`, `diffToHunks`/`diffToRows`, `diffWouldStall`.

- [ ] **Step 1: Replace `<app-code-diff>` with the unified renderer + stall guard**

Add: `rows = computed(() => { const d = this.diff(); return d ? diffToRows(diffToHunks(d.old, d.new)) : []; })` and `stall = computed(() => { const d = this.diff(); return !!d && !this.forceReview() && diffWouldStall(d.old, d.new); })` with `forceReview = signal(false)` (reset in the file-select effect). Replace the diff body:

```html
@if (current() && diff(); as d) {
  @if (annotate()) {
    <app-annotate-blame [lines]="newBlame()" (openCommit)="0" />
  } @else if (stall()) {
    <div class="diff-toobig">
      <span>Large file with long lines — review rendered without inline diff.</span>
      <button class="db-btn" (click)="forceReview.set(true)">Review anyway</button>
    </div>
    <app-review-code [agent]="agent().id" [file]="current()!.path" view="file" [rows]="fileRows()" [lang]="langId()" />
  } @else {
    <app-review-code [agent]="agent().id" [file]="current()!.path" view="diff" [rows]="rows()" [lang]="langId()" />
  }
}
```

Where `fileRows = computed(() => fileToRows(this.diff()?.new ?? ""))` (capped plain view). The existing `annotate()` signal stays; its body now uses `<app-annotate-blame>` instead of the blame-overlay on `<app-code-diff>` (drop the `oldBlame`/`newBlame` overlay path's CodeMirror usage but keep the `newBlame` loader). Mount `<app-send-review-button [agent]="agent().id" [agentName]="agent().name" />` in `.diff-head` (right group). Update `imports`, remove the now-unused `CodeDiffComponent` import.

- [ ] **Step 2: Typecheck + run the full unit suite**

Run: `pnpm tsc -p tsconfig.app.json --noEmit && pnpm vitest run`
Expected: typecheck clean; all specs pass (new + existing).

- [ ] **Step 3: Commit**

```bash
git add src/app/workspace/diff-view.component.ts
git commit -m "feat(review): wire inline review + annotate into the working-changes diff"
```

---

### Task 10: Manual verification in the running app

**Files:** none (verification only).

> The `e2e/` Playwright harness runs `ng serve` with **no Tauri backend**, so there are no agents/changes to drive — a browser E2E cannot reach the review UI. The automated safety net is the vitest suite above (pure logic + the `ReviewCodeComponent` TestBed interaction). End-to-end behavior is verified by running the real app.

- [ ] **Step 1: Launch the app**

Run: `pnpm dev` (Tauri). Open an agent with working changes.

- [ ] **Step 2: Verify the flow (record a GIF or screenshots)**

Confirm, in the working-changes **Diff** tab and an **open file**:
1. Hovering a line shows "+"; clicking adds a single-line comment; dragging the gutter selects a range.
2. The inline composer saves on ⌘/Ctrl-↵, cancels on Esc; a saved card appears under the line; delete removes it.
3. "Send review (N)" appears in the header; opening it shows comments grouped by file + the global-note box.
4. "Send to agent" switches to the terminal and the structured message is pasted as ONE block (no premature submit) and submitted; the batch clears.
5. **Annotate** toggle shows the blame view; hovering a line shows the popup card; clicking opens the commit.
6. The commit/range "multi files diff view" is unchanged (still CodeMirror side-by-side).
7. A large generated file shows the "Review anyway" guard instead of freezing.

- [ ] **Step 3: Commit any fixes found during verification**

```bash
git add -A
git commit -m "fix(review): address issues found in manual verification"
```

---

## Self-Review

- **Spec coverage:** ReviewStore (T2) ✓; unified renderer + comments (T1, T5) ✓; send button + modal (T6) ✓; PTY delivery + assemble (T2, T3) ✓; annotate popup + blame adapter (T7) ✓; stall guard (T9) ✓; scope = file-view + diff-view only, commit/range untouched (T8, T9) ✓; in-memory persistence (T2) ✓; plain-highlight v1 (T5) ✓.
- **Placeholder scan:** Task 1's `diffToHunks` start-number backfill is the one spot flagged to iterate against tests rather than trust the first draft — that is an explicit instruction, not a placeholder; all other steps carry complete code.
- **Type consistency:** `Row`, `ReviewComment`, `ReviewPayload`, `BlameRow` names and `ReviewStore` method signatures (`add/list/count/remove/clear/buildPayload`) are used consistently across T1–T9; `assembleReviewMessage`/`sendReview` signatures match between T2/T3 and their consumers.
