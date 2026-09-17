import { expect, Page, test } from "@playwright/test";

/**
 * E2E for M2 symbol navigation in the Monaco file editor.
 *
 * Backend-free: `agent_diff` is stubbed on AgentsStore (two files of one
 * worktree), and the `nav_*` / `symbols_*` commands on the shared bridge.
 * Monaco's own contributions (F12, peek, hover) run for real; only the
 * provider answers are canned. `symbols://index` is pushed by hand through
 * the recorded `on` handlers, exactly like the extensions spec does.
 */

const AGENT = "e2e-nav";
const USE = "src/use.ts";
const DEF = "src/def.ts";
const USE_TEXT = "import { helper } from './def';\n\nhelper();\nhelper();\n";
const DEF_TEXT = "// def\n\n\nexport function helper() {\n  return 1;\n}\n";

/** 0-based, as the backend speaks. */
const DEF_LOC = { uri: `orrery://${AGENT}/${DEF}`, id: AGENT, path: DEF, line: 3, col: 16, endLine: 3, endCol: 22 };
const USE_LOC = { uri: `orrery://${AGENT}/${USE}`, id: AGENT, path: USE, line: 2, col: 0, endLine: 2, endCol: 6 };

/** The scope service resolves an agent only under a known project, and
 *  Search Everywhere's worktree scope refuses to search without one. */
const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-e2e", name: "e2e-proj", path: "C:/e2e", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main", branches: ["main"],
  });
})()`;

const seedAgent = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${AGENT}", projectId: "p-e2e", tool: "claude", model: "m", name: "e2e-navigator",
    task: "", status: "idle", branch: "agent/nav", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Stub the file reads + the navigation commands; record calls and the
 *  `symbols://index` handlers on `window.__nav`. */
const seedNav = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.agentActions["agentsStore"];
  const texts = { ${JSON.stringify(USE)}: ${JSON.stringify(USE_TEXT)}, ${JSON.stringify(DEF)}: ${JSON.stringify(DEF_TEXT)} };
  store.diff = (id, path) => Promise.resolve({ old: "", new: texts[path] ?? "", lang: "typescript" });
  const bridge = store["bridge"];
  const origInvoke = bridge.invoke.bind(bridge);
  const origOn = bridge.on.bind(bridge);
  const nav = (window.__nav = { calls: [], handlers: [], definition: [] });
  bridge.invoke = (cmd, args) => {
    if (cmd.startsWith("nav_") || cmd.startsWith("symbols_")) nav.calls.push([cmd, args]);
    switch (cmd) {
      case "nav_definition":
        return Promise.resolve({ source: "index", locations: nav.definition, lspState: null });
      case "nav_references":
        return Promise.resolve({ source: "index", locations: [${JSON.stringify(DEF_LOC)}, ${JSON.stringify(USE_LOC)}], lspState: null });
      case "nav_hover":
        return Promise.resolve({ source: "index", contents: "function helper(): number" });
      case "symbols_document":
      case "nav_document_symbols":
        return Promise.resolve([]);
      case "symbols_search":
        return Promise.resolve([
          { name: "helper", kind: "fn", path: ${JSON.stringify(DEF)}, line: 3, col: 16, container: null, agentId: "${AGENT}", root: "e2e-navigator" },
          { name: "helperCount", kind: "const", path: "src/count.ts", line: 0, col: 6, container: "Stats", agentId: "${AGENT}", root: "e2e-navigator" },
        ]);
      case "symbols_index_status":
        return Promise.resolve({ root: args.id, state: "idle", files: 0, done: 0, total: 0, elapsedMs: 0 });
      case "symbols_index_start":
      case "symbols_index_stop":
        return Promise.resolve(null);
      case "agent_file_hunks":
        return Promise.resolve([]);
    }
    return origInvoke(cmd, args);
  };
  bridge.on = (event, handler) => {
    if (event === "symbols://index") {
      nav.handlers.push(handler);
      return Promise.resolve(() => {});
    }
    return origOn(event, handler);
  };
  window.ng.getComponent(document.querySelector("app-status-bar")).index.connect();
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

const setDefinition = (locs: unknown[]) => `window.__nav.definition = ${JSON.stringify(locs)}`;

/** The live editor component showing `file` (there may be several tabs). */
const editorOf = (file: string) => `(() => {
  for (const el of document.querySelectorAll("app-monaco-file-editor")) {
    const c = window.ng.getComponent(el);
    if (c && c.file() === ${JSON.stringify(file)} && c["editor"]) return c;
  }
  return null;
})()`;

async function openUse(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent);
  await page.evaluate(seedNav);
  await page.evaluate(ui(`.openFileInWorkspace("${AGENT}", "${USE}")`));
  await expect(page.locator("app-file-view")).toBeVisible();
  // Monaco arrives as a lazy chunk
  await expect(page.locator("app-monaco-file-editor .monaco-editor")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("app-monaco-file-editor")).toContainText("helper();");
}

/** Put the caret on the `helper` call of line 3 and focus the editor. */
async function caretOnHelper(page: Page): Promise<void> {
  await page.evaluate(`(() => {
    const c = ${editorOf(USE)};
    c["editor"].setPosition({ lineNumber: 3, column: 2 });
    c["editor"].focus();
  })()`);
}

test("F12 on an identifier opens the target file at the definition line", async ({ page }) => {
  await openUse(page);
  await page.evaluate(setDefinition([DEF_LOC]));
  await caretOnHelper(page);
  await page.keyboard.press("F12");

  // the request went out 0-based, for the word under the caret
  await expect
    .poll(() => page.evaluate(`window.__nav.calls.filter(([c]) => c === "nav_definition").map(([, a]) => a)`))
    .toEqual([{ id: AGENT, path: USE, line: 2, col: 1, word: "helper", text: null }]);

  // the opener opened def.ts as its own tab…
  await expect(page.locator("app-file-view").filter({ hasText: "export function helper" })).toBeVisible({ timeout: 15_000 });
  // …with the caret on the definition line (1-based: 0-based 3 → line 4)
  await expect
    .poll(() => page.evaluate(`(() => { const c = ${editorOf(DEF)}; return c ? c["editor"].getPosition().lineNumber : -1; })()`))
    .toBe(4);
  // the index of that root was warmed when the editor mounted
  const starts = await page.evaluate(`window.__nav.calls.filter(([c]) => c === "symbols_index_start").map(([, a]) => a.id)`);
  expect(starts).toContain(AGENT);
});

/** The agent's diff tab with USE listed as a changed file (its "new" side is
 *  USE_TEXT, so the diff surface shows the same code the editor would). */
const seedDiffChanges = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const work = bar.agentActions["work"];
  work["patch"](work["changesMap"], "${AGENT}", {
    status: "ready",
    data: [{ path: ${JSON.stringify(USE)}, add: 3, del: 0, state: "A" }],
  });
})()`;

test("F12 in the agent's DIFF view navigates too — the new side carries the file's id + path", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent);
  await page.evaluate(seedNav);
  await page.evaluate(seedDiffChanges);
  await page.evaluate(setDefinition([DEF_LOC]));
  await page.evaluate(ui(`.openAgent("${AGENT}", "diff")`));
  await expect(page.locator(".diff-head-path")).toContainText(USE);
  await expect(page.locator("app-unified-code .monaco-diff-editor")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("app-unified-code")).toContainText("helper();");

  // caret on the `helper` call in the NEW side, then F12
  await page.evaluate(`(() => {
    const c = window.ng.getComponent(document.querySelector("app-unified-code"));
    const ed = c["diffEditor"].getModifiedEditor();
    ed.setPosition({ lineNumber: 3, column: 2 });
    ed.focus();
  })()`);
  await page.keyboard.press("F12");

  // the request went out for the diff's file, 0-based, WITH the shown text
  await expect
    .poll(() => page.evaluate(`window.__nav.calls.filter(([c]) => c === "nav_definition").map(([, a]) => [a.id, a.path, a.line, a.col, a.word, typeof a.text])`))
    .toEqual([[AGENT, USE, 2, 1, "helper", "string"]]);
  // and the opener opened def.ts as a file tab at the definition line
  await expect(page.locator("app-file-view").filter({ hasText: "export function helper" })).toBeVisible({ timeout: 15_000 });
  await expect
    .poll(() => page.evaluate(`(() => { const c = ${editorOf(DEF)}; return c ? c["editor"].getPosition().lineNumber : -1; })()`))
    .toBe(4);
});

test("two definitions open Monaco's peek instead of jumping", async ({ page }) => {
  await openUse(page);
  await page.evaluate(setDefinition([DEF_LOC, USE_LOC]));
  await caretOnHelper(page);
  await page.keyboard.press("F12");

  const peek = page.locator("app-monaco-file-editor .peekview-widget");
  await expect(peek).toBeVisible({ timeout: 15_000 });
  // the list side names both files; the preview shows the other file's text
  await expect(peek.locator(".ref-tree")).toContainText("def.ts");
  await expect(peek.locator(".ref-tree")).toContainText("use.ts");
  // Monaco preselects the reference nearest the caret (use.ts itself) and
  // keeps the other file's group collapsed: expand def.ts, pick its entry,
  // and the preview swaps to that file's text — its model came from the
  // buffer/diff loader, not from a tab.
  await expect(peek.getByRole("treeitem", { name: /in use\.ts on line/ })).toHaveAttribute("aria-selected", "true");
  await peek.getByRole("treeitem", { name: /1 symbol in def\.ts/ }).click();
  await peek.getByRole("treeitem", { name: /in def\.ts on line/ }).click();
  await expect(peek).toContainText("export function helper");
  // no second tab opened
  await expect(page.locator("app-file-view")).toHaveCount(1);
});

test("no definition → the quiet nav-hint chip, not a tab", async ({ page }) => {
  await openUse(page);
  await page.evaluate(setDefinition([]));
  await caretOnHelper(page);
  await page.keyboard.press("F12");

  const hint = page.locator("app-monaco-file-editor [data-testid=nav-hint]");
  await expect(hint).toBeVisible({ timeout: 15_000 });
  await expect(hint).toContainText("no definition found");
  await expect(page.locator("app-file-view")).toHaveCount(1);
  // it fades on its own
  await expect(hint).toHaveCount(0, { timeout: 10_000 });
});

test("Search Everywhere · Symbols lists index hits as symbol rows", async ({ page }) => {
  await openUse(page);
  await page.keyboard.press("Control+K");
  const se = page.locator("app-search-everywhere");
  await expect(se.locator("input")).toBeVisible();
  await se.getByRole("button", { name: /^Symbols/ }).click();
  await expect(se).toContainText("start typing to search symbols");
  await se.locator("input").fill("hel");

  const rows = se.locator(".sym-row");
  await expect(rows).toHaveCount(2, { timeout: 10_000 });
  // one query, min length 1, backend-side scope
  await expect
    .poll(() => page.evaluate(`window.__nav.calls.filter(([c]) => c === "symbols_search").map(([, a]) => a.query)`))
    .toContain("hel");
  // design SymbolRow: name · container · root label · path:line (1-based)
  const first = rows.filter({ hasText: "helperCount" });
  await expect(first.locator(".ct")).toHaveText("Stats");
  await expect(first.locator(".root")).toHaveText("e2e-navigator");
  await expect(first.locator(".pth")).toHaveText("src/count.ts:1");
  await expect(rows.filter({ hasText: /^helper/ }).first().locator(".pth")).toHaveText(`${DEF}:4`);

  // Enter opens the selected symbol's file at its line
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(se).toHaveCount(0);
  await expect(page.locator("app-file-view").filter({ hasText: "export function helper" })).toBeVisible({ timeout: 15_000 });
});

test("Search Everywhere · Symbols warms the index, shows its progress and re-queries when it is done", async ({ page }) => {
  await openUse(page);
  await page.keyboard.press("Control+K");
  const se = page.locator("app-search-everywhere");
  await expect(se.locator("input")).toBeVisible();
  const indexStarts = () => page.evaluate(`window.__nav.calls.filter(([c]) => c === "symbols_index_start").map(([, a]) => a.id)`);
  const searches = () => page.evaluate(`window.__nav.calls.filter(([c]) => c === "symbols_search").map(([, a]) => a.query)`);
  // opening the editor already warmed this worktree; the tab asks again for
  // every root in scope (idempotent on the backend, deduped per session here)
  const before = (await indexStarts()) as string[];
  await se.getByRole("button", { name: /^Symbols/ }).click();
  await expect(se).toContainText("start typing to search symbols");
  await expect.poll(indexStarts).toEqual(expect.arrayContaining([AGENT]));
  expect(((await indexStarts()) as string[]).length).toBeGreaterThanOrEqual(before.length);

  await se.locator("input").fill("hel");
  await expect.poll(searches).toEqual(["hel"]);
  await expect(se.locator(".sym-row")).toHaveCount(2, { timeout: 10_000 });

  // the backend reports the root still indexing → progress replaces the
  // silent "nothing matches", and the query is asked ONCE more when it ends
  const emit = (payload: unknown) => `window.__nav.handlers.forEach((h) => h(${JSON.stringify(payload)}))`;
  await page.evaluate(emit({ root: AGENT, state: "indexing", files: 0, done: 2341, total: 10020, elapsedMs: 800 }));
  await expect(se).toContainText("indexing 2,341 / 10,020 files…");
  await page.evaluate(emit({ root: AGENT, state: "indexing", files: 0, done: 5000, total: 10020, elapsedMs: 1200 }));
  await expect(se).toContainText("indexing 5,000 / 10,020 files…");
  await page.evaluate(emit({ root: AGENT, state: "ready", files: 10020, done: 10020, total: 10020, elapsedMs: 4000 }));
  await expect(se).not.toContainText("indexing");
  await expect.poll(searches).toEqual(["hel", "hel", "hel"]);
  await page.keyboard.press("Escape");
});

test("footer index chip shows while a root indexes and hides when ready", async ({ page }) => {
  await openUse(page);
  const chip = page.locator("app-status-bar [data-testid=index-chip]");
  await expect(chip).toHaveCount(0);

  const emit = (payload: unknown) => `window.__nav.handlers.forEach((h) => h(${JSON.stringify(payload)}))`;
  await page.evaluate(emit({ root: AGENT, state: "indexing", files: 0, done: 2341, total: 10020, elapsedMs: 800 }));
  await expect(chip).toBeVisible();
  await expect(chip).toContainText("symbols · indexing");
  await expect(chip).toContainText("2,341 / 10,020");
  await expect(chip.locator(".meter")).toBeVisible();

  // a second root → the roots count replaces the numbers
  await page.evaluate(emit({ root: "p-e2e", state: "indexing", files: 0, done: 1, total: 5, elapsedMs: 10 }));
  await expect(chip).toContainText("2 roots");

  await page.evaluate(emit({ root: "p-e2e", state: "ready", files: 5, done: 5, total: 5, elapsedMs: 50 }));
  await page.evaluate(emit({ root: AGENT, state: "ready", files: 10020, done: 10020, total: 10020, elapsedMs: 4000 }));
  await expect(chip).toHaveCount(0);

  // a failed run stays visible, tinted
  await page.evaluate(emit({ root: AGENT, state: "error", files: 0, done: 0, total: 0, elapsedMs: 0, error: "grammar missing" }));
  await expect(chip).toBeVisible();
  await expect(chip).toHaveClass(/error/);
  await expect(chip).toContainText("index failed");
});
