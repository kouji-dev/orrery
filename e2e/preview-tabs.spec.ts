import { expect, Page, test } from "@playwright/test";

/**
 * E2E for v2 preview vs pinned file tabs: a sidebar single-click opens a
 * PREVIEW tab (italic, replaced by the next preview); double-click pins it;
 * pinned tabs survive later previews. Backend-free (tree seeded).
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-e2e", name: "e2e-proj", path: "C:/e2e", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main",
  });
})()`;

const seedAgent = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "e2e-pv1", projectId: "p-e2e", tool: "claude", model: "m", name: "alpha",
    task: "", status: "idle", branch: "agent/alpha", worktree: "C:/wt/alpha", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedTree = `(() => {
  const work = window.ng.getComponent(document.querySelector("app-top-bar")).agentActions["work"];
  work["patch"](work["treesMap"], "e2e-pv1", { status: "ready", data: [
    { name: "a.ts", path: "a.ts", isDir: false, ignored: false, children: null },
    { name: "b.ts", path: "b.ts", isDir: false, ignored: false, children: null },
  ]});
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent);
  await page.evaluate(ui(`.openAgent("e2e-pv1")`));
  await page.evaluate(seedTree);
}

test("single-click opens an italic preview; the next preview replaces it", async ({ page }) => {
  await boot(page);
  const tree = page.locator("app-sidebar-file-tree");
  await tree.getByText("a.ts", { exact: true }).click();
  const strip = page.locator("app-pane-manager .file-strip");
  await expect(strip.locator(".file-tab.preview", { hasText: "a.ts" })).toBeVisible();
  // next preview replaces the a.ts tab
  await tree.getByText("b.ts", { exact: true }).click();
  await expect(strip.locator(".file-tab", { hasText: "b.ts" })).toBeVisible();
  await expect(strip.locator(".file-tab", { hasText: "a.ts" })).toHaveCount(0);
});

test("double-click pins — the tab goes upright and survives later previews", async ({ page }) => {
  await boot(page);
  const tree = page.locator("app-sidebar-file-tree");
  await tree.getByText("a.ts", { exact: true }).dblclick();
  const strip = page.locator("app-pane-manager .file-strip");
  await expect(strip.locator(".file-tab", { hasText: "a.ts" })).toBeVisible();
  await expect(strip.locator(".file-tab.preview", { hasText: "a.ts" })).toHaveCount(0);
  await tree.getByText("b.ts", { exact: true }).click();
  // pinned a.ts is still there next to the b.ts preview
  await expect(strip.locator(".file-tab", { hasText: "a.ts" })).toBeVisible();
  await expect(strip.locator(".file-tab.preview", { hasText: "b.ts" })).toBeVisible();
});

test("double-clicking the preview TAB pins it in place", async ({ page }) => {
  await boot(page);
  const tree = page.locator("app-sidebar-file-tree");
  await tree.getByText("a.ts", { exact: true }).click();
  const strip = page.locator("app-pane-manager .file-strip");
  const tab = strip.locator(".file-tab", { hasText: "a.ts" });
  await expect(tab).toHaveClass(/preview/);
  await tab.dblclick();
  await expect(strip.locator(".file-tab.preview")).toHaveCount(0);
  await expect(tab).toBeVisible();
});

test("Alt+click opens the file into a split — the first leaf stays put", async ({ page }) => {
  await boot(page);
  await page.locator("app-sidebar-file-tree").getByText("a.ts", { exact: true }).click({ modifiers: ["Alt"] });
  // two leaves now: the original view plus the file split
  await expect(page.locator("app-pane-manager .pane-leaf")).toHaveCount(2);
  await expect(page.locator("app-pane-manager .file-strip .file-tab", { hasText: "a.ts" })).toBeVisible();
});
