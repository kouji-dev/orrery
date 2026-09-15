import { expect, Page, test } from "@playwright/test";

/**
 * E2E for M3 virtual read-only documents: a definition that lands OUTSIDE
 * the worktree (a `jdt://…` class-file source from jdtls) opens as a
 * read-only tab — box icon + "library" badge on the tab, the LibDocToolbar
 * with the payload's title and the "read-only · library" badge, the lock
 * banner above the editor, no Annotate, and typing changes nothing.
 *
 * Backend-free, same seeding as goto-definition.spec.ts: `agent_diff` is
 * stubbed on AgentsStore, `nav_definition` answers the jdt location and
 * `nav_virtual_read` the class-file source.
 */

const AGENT = "e2e-vdoc";
const USE = "src/use.ts";
const USE_TEXT = "import { helper } from './def';\n\nhelper();\nhelper();\n";
const JDT = "jdt://contents/java.base/java.util/ArrayList.class?=demo/%5C/usr%5C/lib%5C/jrt-fs.jar%60java.base=/<java.util(ArrayList.class";
const JDT_TEXT = "package java.util;\n\npublic class ArrayList<E> {\n  public boolean add(E e) { return true; }\n}\n";
const JDT_LOC = { uri: JDT, id: "", path: "", line: 2, col: 13, endLine: 2, endCol: 22 };

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
    id: "${AGENT}", projectId: "p-e2e", tool: "claude", model: "m", name: "e2e-vdoc-agent",
    task: "", status: "idle", branch: "agent/vdoc", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedNav = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.agentActions["agentsStore"];
  store.diff = (id, path) => Promise.resolve({ old: "", new: path === ${JSON.stringify(USE)} ? ${JSON.stringify(USE_TEXT)} : "", lang: "typescript" });
  const bridge = store["bridge"];
  const origInvoke = bridge.invoke.bind(bridge);
  const origOn = bridge.on.bind(bridge);
  const nav = (window.__nav = { calls: [], definition: [${JSON.stringify(JDT_LOC)}], lspState: null });
  bridge.invoke = (cmd, args) => {
    if (cmd.startsWith("nav_") || cmd.startsWith("symbols_") || cmd.startsWith("lsp_")) nav.calls.push([cmd, args]);
    switch (cmd) {
      case "nav_definition":
        return Promise.resolve({ source: "lsp", locations: nav.definition, lspState: nav.lspState });
      case "nav_references":
        return Promise.resolve({ source: "index", locations: [], lspState: null });
      case "nav_hover":
        return Promise.resolve(null);
      case "nav_virtual_read":
        return Promise.resolve({ uri: args.uri, language: "java", text: ${JSON.stringify(JDT_TEXT)}, title: "java.util.ArrayList" });
      case "symbols_document":
      case "nav_document_symbols":
        return Promise.resolve([]);
      case "symbols_index_status":
        return Promise.resolve({ root: args.id, state: "idle", files: 0, done: 0, total: 0, elapsedMs: 0 });
      case "symbols_index_start":
      case "symbols_index_stop":
      case "lsp_doc_open":
      case "lsp_doc_change":
      case "lsp_doc_close":
        return Promise.resolve(null);
      case "lsp_status":
        return Promise.resolve({ servers: [] });
      case "agent_file_hunks":
        return Promise.resolve([]);
    }
    return origInvoke(cmd, args);
  };
  bridge.on = (event, handler) => {
    if (event === "symbols://index" || event === "lsp://status") {
      (nav.handlers ||= {})[event] = (nav.handlers[event] || []).concat(handler);
      return Promise.resolve(() => {});
    }
    return origOn(event, handler);
  };
  const sb = window.ng.getComponent(document.querySelector("app-status-bar"));
  sb.index.connect();
  sb.lsp.connect();
})()`;

const ui = (expr: string) => `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

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
  await expect(page.locator("app-monaco-file-editor .monaco-editor")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("app-monaco-file-editor")).toContainText("helper();");
}

async function caretOnHelper(page: Page): Promise<void> {
  await page.evaluate(`(() => {
    const c = ${editorOf(USE)};
    c["editor"].setPosition({ lineNumber: 3, column: 2 });
    c["editor"].focus();
  })()`);
}

test("F12 to a jdt:// location opens a read-only library tab", async ({ page }) => {
  await openUse(page);
  await caretOnHelper(page);
  await page.keyboard.press("F12");

  // the virtual doc was read exactly once, with the raw uri
  await expect.poll(() => page.evaluate(`window.__nav.calls.filter(([c]) => c === "nav_virtual_read").map(([, a]) => a.uri)`)).toEqual([JDT]);

  // the tab: box icon, the entry's file name, the "library" badge, the uri as title
  const tab = page.locator(`.file-tab.virtual`);
  await expect(tab).toBeVisible();
  await expect(tab.locator(".fn")).toHaveText("ArrayList.class");
  await expect(tab.locator(".lib-badge")).toHaveText("library");
  await expect(tab).toHaveAttribute("title", new RegExp("^jdt://contents/"));
  await expect(tab).toHaveClass(/on/);

  // the view: LibDocToolbar with the payload's title + read-only badge, the
  // lock banner, and NO Annotate / reload / review controls
  const view = page.locator("app-file-view").filter({ has: page.locator("[data-testid=lib-bar]") });
  await expect(view).toBeVisible();
  await expect(view.locator("[data-testid=lib-bar] .c.on")).toHaveText("java.util.ArrayList");
  await expect(view.locator("[data-testid=lib-bar] .lib-badge")).toHaveText("read-only · library");
  await expect(view.locator("[data-testid=lib-bar]")).toContainText("java");
  await expect(view.locator("[data-testid=lib-banner]")).toContainText("cannot be edited");
  await expect(view.getByRole("button", { name: /Annotate/ })).toHaveCount(0);
  await expect(view.getByRole("button", { name: /Reload from the worktree/ })).toHaveCount(0);
  await expect(view.locator("app-send-review-button")).toHaveCount(0);

  // the editor shows the source, read-only, caret on the definition line
  const editor = view.locator("app-monaco-file-editor");
  await expect(editor.locator(".monaco-editor")).toBeVisible({ timeout: 15_000 });
  await expect(editor).toContainText("public class ArrayList");
  await expect(editor).toHaveAttribute("data-readonly", "");
  await expect
    .poll(() => page.evaluate(`(() => { const c = ${editorOf(JDT)}; return c ? c["editor"].getPosition().lineNumber : -1; })()`))
    .toBe(3);

  // typing changes nothing (Monaco read-only), and no buffer exists for it
  await page.evaluate(`${editorOf(JDT)}["editor"].focus()`);
  await page.keyboard.type("xyz");
  await page.keyboard.press("Enter");
  expect(await page.evaluate(`${editorOf(JDT)}["editor"].getValue()`)).toBe(JDT_TEXT);
  expect(await page.evaluate(`${editorOf(JDT)}["edits"].get("${AGENT}", ${JSON.stringify(JDT)}) ?? null`)).toBeNull();

  // closing the tab from the strip removes it (no save prompt, no leftovers)
  await tab.hover();
  await tab.locator(".fx").click();
  await expect(page.locator(".file-tab.virtual")).toHaveCount(0);
  await expect(page.locator("app-file-view")).toHaveCount(1);
});

test("a definition answered from the index while the server starts shows the fallback hint", async ({ page }) => {
  await openUse(page);
  await page.evaluate(`window.__nav.lspState = "starting"`);
  // a starting jdtls instance for this project names the hint
  await page.evaluate(`(window.__nav.handlers["lsp://status"] || []).forEach((h) => h(${JSON.stringify({
    servers: [
      { id: "server.jdtls:p-e2e", extId: "server.jdtls", label: "jdtls", language: "javascript", root: "C:/e2e", projectId: "p-e2e", projectName: "e2e-proj", pid: null, state: "starting", memBytes: 0, cpu: 0, restarts: 0, startedAt: null, lastError: null },
    ],
  })}))`);
  await caretOnHelper(page);
  await page.keyboard.press("F12");
  const hint = page.locator("app-monaco-file-editor [data-testid=nav-hint]");
  await expect(hint).toBeVisible({ timeout: 15_000 });
  await expect(hint).toHaveAttribute("data-kind", "fallback");
  await expect(hint).toContainText("jdtls starting… showing index result");
  await expect(hint.locator("kj-spinner")).toBeVisible();
  // the index hit still opened
  await expect(page.locator(".file-tab.virtual")).toBeVisible();
  // and the hint fades on its own
  await expect(hint).toHaveCount(0, { timeout: 10_000 });
});
