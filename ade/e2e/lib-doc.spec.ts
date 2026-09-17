import { expect, Page, test } from "@playwright/test";

/**
 * E2E for M4 library sources — automatic and invisible: the footer
 * IndexChip's library copy fed by `libsrc://status`, the dev console's
 * library-index lines (Projects tab: one per source, Re-index →
 * `libsrc_reindex`, Re-scan projects → `libsrc_rescan`), the Extensions
 * panel WITHOUT a Library sources section, and F12 to an `orrery-lib://`
 * location opening the read-only tab with the design LibDocToolbar crumbs
 * ("JDK 21 · java.base · java.util") and the library badge.
 *
 * Backend-free, same seeding as virtual-doc.spec.ts: `agent_diff` is stubbed
 * on AgentsStore, the `libsrc_*` / `nav_*` commands on the shared bridge, and
 * the stores re-attach (`connect()`) so `libsrc://status` can be pushed by
 * hand through the recorded handlers.
 */

const AGENT = "e2e-lib";
const USE = "src/use.ts";
const USE_TEXT = "import { helper } from './def';\n\nhelper();\nhelper();\n";
const LIB = "orrery-lib://jdk1/java.base/java/util/ArrayList.java";
const LIB_TEXT = "package java.util;\n\npublic class ArrayList<E> {\n  public boolean add(E e) { return true; }\n}\n";
const LIB_TITLE = "ArrayList.java — JDK 21 (java.base)";
const LIB_LOC = { uri: LIB, id: null, path: null, line: 2, col: 13, endLine: 2, endCol: 22, kind: "class", preview: "java.util.ArrayList" };

const MB = 1024 * 1024;
const SOURCES = [
  { id: "jdk1", kind: "jdk", path: "C:/jdk-21/lib/src.zip", label: "JDK 21 (openjdk21)", state: "done", files: 23010, decls: 201340, indexedAt: Date.now() - 3_600_000, sizeBytes: 25 * MB, projectId: null, projectName: null, artifacts: 1, missing: 0, skipped: 0, error: null },
  { id: "cargo1", kind: "cargo", path: "C:/e2e/src-tauri/Cargo.lock", label: "Cargo · orrery", state: "indexing", files: 0, decls: 0, indexedAt: null, sizeBytes: 0, projectId: "p-e2e", projectName: "e2e-proj", artifacts: 647, missing: 0, skipped: 2, error: null, done: 2341, total: 23010 },
];
const EMPTY_REGISTRY = { registryUrl: "https://example.test/index.json", fetchedAt: Date.now() - 120_000, offline: false, items: [] };

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
    id: "${AGENT}", projectId: "p-e2e", tool: "claude", model: "m", name: "e2e-lib-agent",
    task: "", status: "idle", branch: "agent/lib", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Stub the file read, the `libsrc_*` + `nav_*` commands; record calls and
 *  every event handler on `window.__lib`; re-attach the stores. */
const seedLib = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.agentActions["agentsStore"];
  store.diff = (id, path) => Promise.resolve({ old: "", new: path === ${JSON.stringify(USE)} ? ${JSON.stringify(USE_TEXT)} : "", lang: "typescript" });
  const bridge = store["bridge"];
  const origInvoke = bridge.invoke.bind(bridge);
  const lib = (window.__lib = { calls: [], handlers: {}, sources: ${JSON.stringify(SOURCES)} });
  bridge.invoke = (cmd, args) => {
    if (/^(nav_|symbols_|lsp_|libsrc_|ext_)/.test(cmd)) lib.calls.push([cmd, args]);
    switch (cmd) {
      case "libsrc_sources":
        return Promise.resolve(lib.sources);
      case "libsrc_ensure":
      case "libsrc_rescan":
      case "libsrc_reindex":
      case "libsrc_cancel":
      case "libsrc_remove":
        return Promise.resolve(null);
      case "ext_registry_list":
        return Promise.resolve(${JSON.stringify(EMPTY_REGISTRY)});
      case "nav_definition":
        return Promise.resolve({ source: "index", locations: [${JSON.stringify(LIB_LOC)}], lspState: null });
      case "nav_references":
        return Promise.resolve({ source: "index", locations: [], lspState: null });
      case "nav_hover":
        return Promise.resolve(null);
      case "nav_virtual_read":
        return Promise.resolve({ uri: args.uri, language: "java", text: ${JSON.stringify(LIB_TEXT)}, title: ${JSON.stringify(LIB_TITLE)} });
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
    (lib.handlers[event] ||= []).push(handler);
    return Promise.resolve(() => {});
  };
  const sb = window.ng.getComponent(document.querySelector("app-status-bar"));
  sb.index.connect();
  sb.lsp.connect();
  sb.libsrc.connect();
  bar.extensions.connect();
})()`;

const emit = (event: string, payload: unknown) =>
  `(window.__lib.handlers[${JSON.stringify(event)}] || []).forEach((h) => h(${JSON.stringify(payload)}))`;

const ui = (expr: string) => `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

/** The live editor component showing `file` (there may be several tabs). */
const editorOf = (file: string) => `(() => {
  for (const el of document.querySelectorAll("app-monaco-file-editor")) {
    const c = window.ng.getComponent(el);
    if (c && c.file() === ${JSON.stringify(file)} && c["editor"]) return c;
  }
  return null;
})()`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent);
  await page.evaluate(seedLib);
}

async function openUse(page: Page): Promise<void> {
  await boot(page);
  await page.evaluate(ui(`.openFileInWorkspace("${AGENT}", "${USE}")`));
  await expect(page.locator("app-file-view")).toBeVisible();
  await expect(page.locator("app-monaco-file-editor .monaco-editor")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("app-monaco-file-editor")).toContainText("helper();");
}

const dialog = (page: Page) => page.locator("app-extensions-modal[role=dialog]");
const srow = (page: Page, id: string) => dialog(page).locator(`.ext-row[data-src-id="${id}"]`);
const chip = (page: Page) => page.locator("app-status-bar [data-testid=index-chip]");
const norm = (s: string | null) => (s ?? "").replace(/\s+/g, " ").trim();

test("libsrc://status drives the footer chip; the dev console lists the sources (Re-index / Re-scan); no Library sources section", async ({ page }) => {
  await boot(page);
  // the seed already has the project's cargo source indexing → the footer chip says so
  await expect(chip(page)).toBeVisible();
  await expect(chip(page)).toHaveAttribute("data-mode", "library");
  expect(norm(await chip(page).textContent())).toBe("library · indexing Cargo · orrery · 2,341 / 23,010");

  // the Extensions panel: grammars, servers, updates — nothing about sources
  await page.locator(".tb-extensions").click();
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).locator(".set-nav-item")).toHaveText(["Grammars", "Language servers", "Updates"]);
  await expect(dialog(page)).not.toContainText("Library sources");
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveCount(0);

  // …a progress tick from the backend
  await page.evaluate(emit("libsrc://status", { sourceId: "cargo1", label: "Cargo · orrery", kind: "cargo", state: "indexing", done: 12000, total: 23010, decls: 90000 }));
  await expect.poll(async () => norm(await chip(page).textContent())).toBe("library · indexing Cargo · orrery · 12,000 / 23,010");

  // the dev console (Projects tab): one status line per source
  await page.locator(".sb-chip", { hasText: "Dev" }).click();
  const panel = page.locator(".dvcon");
  await expect(panel).toBeVisible();
  await panel.locator(".dvc-tab", { hasText: "Projects" }).click();
  const lib = panel.locator("[data-testid=lib-index]");
  await expect(lib.locator("[data-src-id]")).toHaveCount(2);
  await expect(lib.locator("[data-src-id=jdk1] .dvc-fp")).toHaveText("JDK 21 (openjdk21) · indexed · 23k files · 201k decls");
  await expect(lib.locator("[data-src-id=cargo1] .dvc-fp")).toHaveText("Cargo · orrery · 647 crates · 2 skipped · indexing 12,000 / 23,010");
  await expect(lib.locator("[data-src-id=cargo1]").getByRole("button", { name: "Re-index" })).toBeDisabled();

  // …then done: the chip leaves, the line settles
  await page.evaluate(emit("libsrc://status", { sourceId: "cargo1", label: "Cargo · orrery", kind: "cargo", state: "done", done: 23010, total: 23010, decls: 180000 }));
  await expect(chip(page)).toHaveCount(0);
  await expect(lib.locator("[data-src-id=cargo1] .dvc-fp")).toHaveText("Cargo · orrery · 647 crates · 2 skipped · indexed · 23k files · 180k decls");
  await expect(lib.locator("[data-src-id=cargo1]").getByRole("button", { name: "Re-index" })).toBeEnabled();

  // Re-index: the command goes out, the row flips at once, the chip names the source
  await lib.locator("[data-src-id=jdk1]").getByRole("button", { name: "Re-index" }).click();
  await expect
    .poll(() => page.evaluate(`window.__lib.calls.filter(([c]) => c === "libsrc_reindex").map(([, a]) => a)`))
    .toEqual([{ sourceId: "jdk1" }]);
  await expect(lib.locator("[data-src-id=jdk1]")).toHaveAttribute("data-state", "indexing");
  await expect.poll(async () => norm(await chip(page).textContent())).toBe("library · indexing JDK 21 (openjdk21)");

  // Re-scan projects → libsrc_rescan
  await lib.getByRole("button", { name: "Re-scan projects" }).click();
  await expect.poll(() => page.evaluate(`window.__lib.calls.filter(([c]) => c === "libsrc_rescan").length`)).toBe(1);
});

test("F12 to an orrery-lib:// location opens the read-only tab with the library crumbs", async ({ page }) => {
  await openUse(page);
  // the editor open asked the backend for the root's library sources
  await expect.poll(() => page.evaluate(`window.__lib.calls.filter(([c]) => c === "libsrc_ensure").map(([, a]) => a)`)).toEqual([{ id: AGENT }]);

  await page.evaluate(`(() => {
    const c = ${editorOf(USE)};
    c["editor"].setPosition({ lineNumber: 3, column: 2 });
    c["editor"].focus();
  })()`);
  await page.keyboard.press("F12");

  await expect.poll(() => page.evaluate(`window.__lib.calls.filter(([c]) => c === "nav_virtual_read").map(([, a]) => a.uri)`)).toEqual([LIB]);

  // the tab (design LibDocTab): box icon, file name, "library" badge
  const tab = page.locator(".file-tab.virtual");
  await expect(tab).toBeVisible();
  await expect(tab.locator(".fn")).toHaveText("ArrayList.java");
  await expect(tab.locator(".lib-badge")).toHaveText("library");
  await expect(tab).toHaveAttribute("title", LIB);

  // the toolbar (design LibDocToolbar): crumbs from the title + entry path,
  // the file name last, the read-only badge, the language
  const view = page.locator("app-file-view").filter({ has: page.locator("[data-testid=lib-bar]") });
  await expect(view).toBeVisible();
  const bar = view.locator("[data-testid=lib-bar]");
  await expect(bar.locator(".c:not(.on)")).toHaveText(["JDK 21", "java.base", "java.util"]);
  await expect(bar.locator(".c.on")).toHaveText("ArrayList.java");
  await expect(bar.locator(".c.on")).toHaveAttribute("title", `${LIB_TITLE}\njava.util.ArrayList`);
  await expect(bar.locator(".lib-badge")).toHaveText("read-only · library");
  await expect(bar).toContainText("java");
  await expect(view.locator("[data-testid=lib-banner]")).toContainText("cannot be edited");
  await expect(view.getByRole("button", { name: /Annotate/ })).toHaveCount(0);

  // the source, read-only, caret on the definition line
  const editor = view.locator("app-monaco-file-editor");
  await expect(editor.locator(".monaco-editor")).toBeVisible({ timeout: 15_000 });
  await expect(editor).toContainText("public class ArrayList");
  await expect(editor).toHaveAttribute("data-readonly", "");
  await expect
    .poll(() => page.evaluate(`(() => { const c = ${editorOf(LIB)}; return c ? c["editor"].getPosition().lineNumber : -1; })()`))
    .toBe(3);
  await page.evaluate(`${editorOf(LIB)}["editor"].focus()`);
  await page.keyboard.type("xyz");
  expect(await page.evaluate(`${editorOf(LIB)}["editor"].getValue()`)).toBe(LIB_TEXT);
});
