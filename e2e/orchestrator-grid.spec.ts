import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the orchestrator GRID: the recency segments (This week / This month /
 * Older, bucketed by an agent's last run) and the action-row button sizing.
 *
 * The height assertion is the regression test for the shrunken "open worktree
 * folder" button: kouji's `icon` size is deliberately stripped of its control
 * height for dense toolbars (styles.css), which made a lone glyph render half as
 * tall as the labelled controls beside it in a card.
 *
 * Backend-free: agents are seeded straight into the store.
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-og", name: "og-proj", path: "C:/og", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main",
  });
})()`;

/** `agoDays` back from now; pass null for an agent that has never run. */
const seedAgent = (id: string, name: string, agoDays: number | null) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-og", tool: "claude", model: "m", name: "${name}",
    task: "do a thing", status: "idle", branch: "agent/${name}",
    worktree: "C:/wt/${name}", base: "main", started: true,
    commits: 0, elapsed: 0, progress: 0, pending: [],
    lastRunAt: ${agoDays === null ? "undefined" : `Date.now() - ${agoDays} * 86400000`},
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent("og-week", "freshy", 2));
  await page.evaluate(seedAgent("og-month", "midway", 12));
  await page.evaluate(seedAgent("og-old", "ancient", 90));
  await page.evaluate(seedAgent("og-never", "unrun", null));
  await page.evaluate(ui(`.selectTab("orchestrator")`));
  await page.evaluate(ui(`.viz.set("grid")`));
  await page.waitForSelector("app-grid-view");
}

test("recency segments bucket agents by last run and default to this week", async ({ page }) => {
  await boot(page);
  const grid = page.locator("app-grid-view");

  // counts ride on the tab labels; an agent that never ran counts as Older
  await expect(grid.getByRole("tab", { name: /This week · 1/ })).toBeVisible();
  await expect(grid.getByRole("tab", { name: /This month · 1/ })).toBeVisible();
  await expect(grid.getByRole("tab", { name: /Older · 2/ })).toBeVisible();

  // default bucket is this week — only the recent agent's card is rendered
  await expect(grid.locator("app-agent-card")).toHaveCount(1);
  await expect(grid).toContainText("freshy");
  await expect(grid).not.toContainText("ancient");
});

test("switching a segment swaps which agents the grid shows", async ({ page }) => {
  await boot(page);
  const grid = page.locator("app-grid-view");

  await grid.getByRole("tab", { name: /Older/ }).click();
  await expect(grid.locator("app-agent-card")).toHaveCount(2);
  await expect(grid).toContainText("ancient");
  await expect(grid).toContainText("unrun");
  await expect(grid).not.toContainText("freshy");

  await grid.getByRole("tab", { name: /This month/ }).click();
  await expect(grid).toContainText("midway");
});

test("an empty bucket is unpickable instead of showing an empty grid", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  // nothing recent at all — the default bucket must not strand the user
  await page.evaluate(seedAgent("og-old-only", "ancient", 120));
  await page.evaluate(ui(`.selectTab("orchestrator")`));
  await page.evaluate(ui(`.viz.set("grid")`));

  const grid = page.locator("app-grid-view");
  await expect(grid.getByRole("tab", { name: /This week · 0/ })).toBeDisabled();
  // …it falls back to the first non-empty bucket rather than rendering nothing
  await expect(grid.locator("app-agent-card")).toHaveCount(1);
  await expect(grid).toContainText("ancient");
});

test("card action-row buttons all share one control height", async ({ page }) => {
  await boot(page);
  const row = page.locator("app-agent-card").first().locator("div").filter({
    has: page.getByTitle("Open worktree folder"),
  }).last();

  const buttons = row.locator("button.kj-button");
  const n = await buttons.count();
  expect(n).toBeGreaterThan(1);

  const heights: number[] = [];
  for (let i = 0; i < n; i++) {
    const box = await buttons.nth(i).boundingBox();
    expect(box).not.toBeNull();
    heights.push(Math.round(box!.height));
  }
  // the icon-only folder button must not read as a shrunken sibling
  expect(new Set(heights).size, `action row heights: ${heights.join(", ")}`).toBe(1);
});
