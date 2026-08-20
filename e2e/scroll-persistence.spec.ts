import { expect, Page, test } from "@playwright/test";

/**
 * E2E for scroll stability:
 *  1. the Files tree must NOT unmount its virtual-scroll viewport (flicker,
 *     lost offset) when a watcher scan re-fetches the tree in the background;
 *  2. a file tab's scroll position survives switching to another file tab and
 *     back (ScrollStateService, keyed agent:path, evicted on tab close).
 *
 * Backend-free, same seeding pattern as file-tree / markdown-preview specs.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

// ---- 1. tree stays mounted through a background rescan ----

const seedTree = (id: string, count: number) => `(() => {
  const tree = window.ng.getComponent(document.querySelector("app-file-tree"));
  const nodes = Array.from({ length: ${count} }, (_, i) => ({
    name: "file-" + String(i).padStart(3, "0") + ".ts",
    path: "src/file-" + String(i).padStart(3, "0") + ".ts",
    isDir: false, ignored: false,
  }));
  tree["work"]["patch"](tree["work"]["treesMap"], "${id}", { status: "ready", data: nodes });
})()`;

/** Watcher-scan shape: AgentTree hangs → the store sits in "loading". */
const startHangingRescan = (id: string) => `(() => {
  const tree = window.ng.getComponent(document.querySelector("app-file-tree"));
  const work = tree["work"];
  const bridge = work["bridge"];
  const orig = bridge.invoke.bind(bridge);
  bridge.invoke = (cmd, args) =>
    cmd === "agent_tree" ? new Promise(() => {}) : orig(cmd, args);
  work.loadTree("${id}");
})()`;

test("files tree keeps its viewport and scroll offset through a rescan", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-sp1", "e2e-scroll-tree"));
  await page.evaluate(ui(`.openAgent("e2e-sp1")`));
  await page.locator("app-right-panel").getByRole("button", { name: "Files" }).click();
  await expect(page.locator("app-file-tree")).toBeVisible();

  await page.evaluate(seedTree("e2e-sp1", 80));
  const viewport = page.locator("app-file-tree cdk-virtual-scroll-viewport");
  await expect(viewport).toBeVisible();

  await viewport.evaluate((el) => (el.scrollTop = 400));
  await expect.poll(() => viewport.evaluate((el) => el.scrollTop)).toBeGreaterThan(300);

  await page.evaluate(startHangingRescan("e2e-sp1"));

  // loading is live (header chip) but the body must not swap to the scanning
  // placeholder — the viewport stays mounted with its offset intact
  await expect(page.locator("app-file-tree .tnum", { hasText: "scanning…" })).toBeVisible();
  await expect(viewport).toBeVisible();
  await expect(page.getByText("scanning worktree…")).toHaveCount(0);
  expect(await viewport.evaluate((el) => el.scrollTop)).toBeGreaterThan(300);
});

// ---- 2. file tab scroll position survives tab switches ----

const LONG_MD = "# Long Doc\n\n" + "lorem ipsum paragraph body text\n\n".repeat(300);

const SHORT_MD = "# Short Doc\n\nnothing to scroll here\n";

const seedMd = () => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = (id, path) => Promise.resolve({
    old: "",
    new: path === "docs/a.md" ? ${JSON.stringify(LONG_MD)} : ${JSON.stringify(SHORT_MD)},
  });
})()`;

async function openMdTabs(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-sp2", "e2e-scroll-tabs"));
  await page.evaluate(seedMd());
  await page.evaluate(ui(`.openFileInWorkspace("e2e-sp2", "docs/a.md")`));
  await expect(page.locator(".md-body h1", { hasText: "Long Doc" })).toBeVisible();
}

test("markdown tab restores its scroll position after switching tabs and back", async ({ page }) => {
  await openMdTabs(page);
  const body = page.locator(".md-body");
  await body.evaluate((el) => (el.scrollTop = 600));
  await expect.poll(() => body.evaluate((el) => el.scrollTop)).toBeGreaterThan(500);

  // second file in the same leaf — same app-file-view instance, path switch
  await page.evaluate(ui(`.openFileInWorkspace("e2e-sp2", "docs/b.md")`));
  await expect.poll(() => body.evaluate((el) => el.scrollTop)).toBeLessThan(100);

  await page.evaluate(ui(`.openFileInWorkspace("e2e-sp2", "docs/a.md")`));
  await expect.poll(() => body.evaluate((el) => el.scrollTop)).toBeGreaterThan(500);
});

// ---- 3. diff tab: selected file + diff scroll survive a tab switch ----

const LONG_TS = Array.from({ length: 400 }, (_, i) => `const line${i} = ${i};`).join("\n");

/** Stub the diff invoke + seed two changed files (a.ts short, b.ts long). */
const seedDiffChanges = (id: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = (agentId, path) => Promise.resolve(
    path === "src/b.ts"
      ? { old: "", new: ${JSON.stringify(LONG_TS)} }
      : { old: "const a = 1;\\n", new: "const a = 2;\\n" },
  );
  const work = bar.agentActions["work"];
  work["patch"](work["changesMap"], "${id}", {
    status: "ready",
    data: [
      { path: "src/a.ts", add: 1, del: 1, state: "M" },
      { path: "src/b.ts", add: 400, del: 0, state: "A" },
    ],
  });
})()`;

const diffEditor = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-unified-code"))["diffEditor"]${expr}`;

test("diff tab keeps its selected file and scroll after switching tabs and back", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-sp3", "e2e-scroll-diff"));
  await page.evaluate(seedDiffChanges("e2e-sp3"));
  await page.evaluate(ui(`.openAgent("e2e-sp3", "diff")`));

  // default selection is the first changed file; pick the second one
  await expect(page.locator(".diff-head-path")).toContainText("src/a.ts");
  await page.locator(".diff-file", { hasText: "b.ts" }).click();
  await expect(page.locator(".diff-head-path")).toContainText("src/b.ts");

  // scroll the Monaco diff once it's live
  await expect(page.locator("app-unified-code .monaco-diff-editor")).toBeVisible();
  await expect
    .poll(() => page.evaluate(diffEditor(`.getModifiedEditor().getScrollHeight()`)))
    .toBeGreaterThan(1000);
  await page.evaluate(diffEditor(`.getModifiedEditor().setScrollTop(800)`));

  // away to the orchestrator tab (destroys the pane + diff view) and back
  await page.evaluate(ui(`.activeTab.set("orchestrator")`));
  await expect(page.locator(".diff-head")).toHaveCount(0);
  await page.evaluate(ui(`.openAgent("e2e-sp3", "diff")`));

  // the opened file survives, and the diff restores its scroll position
  await expect(page.locator(".diff-head-path")).toContainText("src/b.ts");
  await expect(page.locator("app-unified-code .monaco-diff-editor")).toBeVisible();
  await expect
    .poll(() => page.evaluate(diffEditor(`.getModifiedEditor().getScrollTop()`)))
    .toBeGreaterThan(600);
});

test("closing the file tab drops the saved position — reopen starts at top", async ({ page }) => {
  await openMdTabs(page);
  const body = page.locator(".md-body");
  await body.evaluate((el) => (el.scrollTop = 600));
  await expect.poll(() => body.evaluate((el) => el.scrollTop)).toBeGreaterThan(500);

  // close the file tab through its strip close button, then reopen the path
  await page.locator('.file-tab[data-path="docs/a.md"] .fx').click();
  await expect(page.locator(".md-body")).toHaveCount(0);

  await page.evaluate(ui(`.openFileInWorkspace("e2e-sp2", "docs/a.md")`));
  await expect(page.locator(".md-body h1", { hasText: "Long Doc" })).toBeVisible();
  await page.waitForTimeout(200); // restore (if any) runs within two frames
  expect(await body.evaluate((el) => el.scrollTop)).toBeLessThan(100);
});
