import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the sidebar FILES panel's visual parity with the design's QuietRows
 * (design/orrery-v2.html): mono 13px rows about 21px tall, hover = panel-2,
 * the open file = panel-3 ground, dirs one ink step quieter with a trailing
 * slash. Backend-free: tree data is seeded straight into AgentWorkStore.
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
    id: "e2e-par1", projectId: "p-e2e", tool: "claude", model: "m", name: "parity",
    task: "", status: "idle", branch: "agent/parity", worktree: "C:/wt/parity", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedTree = `(() => {
  const work = window.ng.getComponent(document.querySelector("app-top-bar")).agentActions["work"];
  work["patch"](work["treesMap"], "e2e-par1", { status: "ready", data: [
    { name: "src", path: "src", isDir: true, ignored: false, children: [
      { name: "main.ts", path: "src/main.ts", isDir: false, ignored: false, children: null },
      { name: "app.ts", path: "src/app.ts", isDir: false, ignored: false, children: null },
    ] },
    { name: "readme.md", path: "readme.md", isDir: false, ignored: false, children: null },
  ]});
  work["patch"](work["changesMap"], "e2e-par1", { status: "ready", data: [] });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

/** Computed background a probe element gets from a token — what a row must match. */
const tokenBg = (token: string) => `(() => {
  const p = document.createElement("div");
  p.style.background = "var(${token})";
  document.body.appendChild(p);
  const v = getComputedStyle(p).backgroundColor;
  p.remove();
  return v;
})()`;

const tokenPx = (token: string) =>
  `parseFloat(getComputedStyle(document.documentElement).getPropertyValue("${token}"))`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent);
  await page.evaluate(ui(`.openAgent("e2e-par1")`));
  await page.evaluate(seedTree);
  await expect(page.locator("app-sidebar-file-tree .tree-row").first()).toBeVisible();
}

test("rows are mono 13px, ~21px tall — the design's quiet rows, not 16px chrome text", async ({ page }) => {
  await boot(page);
  const row = page.locator("app-sidebar-file-tree .tree-row").first();
  const m = await row.evaluate((el) => {
    const cs = getComputedStyle(el);
    return { fs: parseFloat(cs.fontSize), h: el.getBoundingClientRect().height, font: cs.fontFamily };
  });
  expect(m.fs).toBe(await page.evaluate(tokenPx("--fs-2xs")));
  expect(m.h).toBe(await page.evaluate(tokenPx("--tree-row-h")));
  expect(m.font).toMatch(/JetBrains Mono|monospace/);
});

test("dirs read quieter with a trailing slash; files carry no slash", async ({ page }) => {
  await boot(page);
  const tree = page.locator("app-sidebar-file-tree");
  const dir = tree.locator(".tree-row.dir").first();
  await expect(dir).toContainText("src/");
  await expect(tree.locator(".tree-row", { hasText: "readme.md" })).not.toHaveClass(/dir/);
  const dirColor = await dir.evaluate((el) => getComputedStyle(el).color);
  const fileColor = await tree
    .locator(".tree-row", { hasText: "readme.md" })
    .evaluate((el) => getComputedStyle(el).color);
  expect(dirColor).not.toBe(fileColor);
});

test("hover paints the shared panel-2 ground", async ({ page }) => {
  await boot(page);
  const row = page.locator("app-sidebar-file-tree .tree-row", { hasText: "readme.md" });
  const before = await row.evaluate((el) => getComputedStyle(el).backgroundColor);
  await row.hover();
  const hovered = await page.evaluate(tokenBg("--panel-2"));
  await expect.poll(() => row.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(hovered);
  expect(before).not.toBe(hovered);
});

test("the open file is the one selected row: panel-3 ground, ink text", async ({ page }) => {
  await boot(page);
  const tree = page.locator("app-sidebar-file-tree");
  const row = tree.locator(".tree-row", { hasText: "readme.md" });
  await row.click(); // preview-opens the file in this root's workspace
  await expect(row).toHaveClass(/sel/);
  await page.mouse.move(0, 0); // selected outranks hover — read it unhovered
  const sel = await page.evaluate(tokenBg("--panel-3"));
  await expect.poll(() => row.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(sel);
  // exactly one selected row, and dirs never take it
  await expect(tree.locator(".tree-row.sel")).toHaveCount(1);
});
