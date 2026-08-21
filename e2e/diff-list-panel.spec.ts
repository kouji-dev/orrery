import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the agent diff panel's file list:
 *  1. Tree and Flat rows share ONE height (--row-h, density-scaled) — the flat
 *     view's directory rides inline instead of a second line;
 *  2. the base width is 300px;
 *  3. a user-resized width persists across a relaunch (WorkspaceStore doc).
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedChanges = (id: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: "x\\n" });
  const work = bar.agentActions["work"];
  work["patch"](work["changesMap"], "${id}", {
    status: "ready",
    data: [
      { path: "src/app/deep/nested/example.ts", add: 3, del: 1, state: "M" },
      { path: "docs/guide.md", add: 5, del: 0, state: "A" },
      { path: "renamed.ts", oldPath: "old-name.ts", add: 1, del: 1, state: "R" },
    ],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openDiff(page: Page, agentId: string): Promise<void> {
  await page.evaluate(seedAgent(agentId, "e2e-diff-list"));
  await page.evaluate(seedChanges(agentId));
  await page.evaluate(ui(`.openAgent("${agentId}", "diff")`));
  await expect(page.locator(".diff-head")).toBeVisible();
}

const rowHeights = (page: Page) =>
  page.$$eval(".diff-file", (els) => els.map((el) => (el as HTMLElement).offsetHeight));

test("tree and flat rows share one density-derived height", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await openDiff(page, "e2e-dl1");

  // tree mode (default): every file row is exactly --row-h tall. The token is
  // a calc(), so resolve it through a probe element.
  const rowH = await page.evaluate(`(() => {
    const probe = document.createElement("div");
    probe.style.height = "var(--row-h)";
    document.body.appendChild(probe);
    const h = probe.getBoundingClientRect().height;
    probe.remove();
    return Math.round(h);
  })()`);
  expect(rowH).toBeGreaterThan(0);
  const treeHeights = await rowHeights(page);
  expect(treeHeights.length).toBeGreaterThan(0);
  for (const h of treeHeights) expect(Math.abs(h - (rowH as number))).toBeLessThanOrEqual(1);

  // flat mode: same rows, same height — dir/rename info inline, not stacked
  await page.getByRole("button", { name: "Flat" }).click();
  const flatHeights = await rowHeights(page);
  expect(flatHeights).toHaveLength(3);
  for (const h of flatHeights) expect(Math.abs(h - (rowH as number))).toBeLessThanOrEqual(1);
  await expect(page.locator(".diff-file .fdir").first()).toBeVisible();
});

test("the list width defaults to 300 and a resize persists across a relaunch", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await openDiff(page, "e2e-dl2");

  const columns = () =>
    page.locator(".diff-grid").evaluate((el) => getComputedStyle(el).gridTemplateColumns);
  expect(await columns()).toMatch(/^300px /);

  // the user resizes (store-backed, same path the separator drag writes)
  await page.evaluate(ui(`.diffListWidth.set(432)`));
  expect(await columns()).toMatch(/^432px /);

  // debounced persistence lands in the workspace doc, then survives a reload
  await expect
    .poll(() => page.evaluate(`localStorage.getItem("orrery.workspace") ?? ""`), { timeout: 5000 })
    .toContain('"diffListWidth":432');
  await page.reload();
  await page.waitForSelector("app-top-bar");
  await openDiff(page, "e2e-dl2b");
  expect(await columns()).toMatch(/^432px /);

  // double-click on the separator resets to the default
  await page.locator(".diff-resizer").dblclick();
  expect(await columns()).toMatch(/^300px /);
});
