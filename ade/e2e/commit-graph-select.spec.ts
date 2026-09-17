import { expect, Page, test } from "@playwright/test";

/**
 * Multi-select in the Git Graph panel: click picks one, ctrl-click toggles one,
 * shift-click selects the contiguous run between the last plain/ctrl-clicked
 * row and the clicked row, ADDED to what is already picked. Backend-free — commits are seeded into the work
 * store, the panel is opened from the graph strip.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 5, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedCommits = (id: string, n: number) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const work = bar.agentActions["work"];
  const data = Array.from({ length: ${n} }, (_, i) => ({
    agent: "e2e", projectId: "p-e2e", sha: "c" + i + "000000", msg: "commit " + i,
    when: "1m", files: 1, ts: Math.floor(Date.now() / 1000) - i,
  }));
  work["patch"](work["commitsMap"], "${id}", { status: "ready", data, hasMore: false });
})()`;

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-e2e", name: "e2e-proj", path: "", icon: "box", color: "#a855f7",
    folderExists: true, hasGit: true, branch: "main", branches: ["main"],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openGraph(page: Page, id: string): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent(id, "sel"));
  await page.evaluate(ui(`.openAgent("${id}")`));
  await page.locator("app-graph-strip").click();
  await expect(page.locator("app-commit-graph-panel")).toBeVisible();
  // the panel's own fetch rejects (no backend) and writes an empty error entry
  // when it lands — wait for that so the seed below is never overwritten
  await expect(page.locator("app-commit-graph-panel")).toContainText("no commits on this branch yet");
  await page.evaluate(seedCommits(id, 5));
  await expect(rows(page)).toHaveCount(5);
}

const rows = (page: Page) => page.locator("app-commit-graph-panel .row-hover");

const selected = (page: Page) =>
  page.evaluate(() => {
    const panel = (window as any).ng.getComponent(document.querySelector("app-commit-graph-panel"));
    return panel.sel() as string[];
  });

test("shift-click adds the run from the last ctrl-clicked row, keeping earlier picks", async ({ page }) => {
  await openGraph(page, "e2e-cg1");
  await rows(page).nth(0).click();
  await rows(page).nth(2).click({ modifiers: ["Control"] });
  expect(await selected(page)).toEqual(["c0000000", "c2000000"]);

  // the run stretches from the ctrl-clicked row; row 0 is not between them yet stays
  await rows(page).nth(4).click({ modifiers: ["Shift"] });
  expect(await selected(page)).toEqual(["c0000000", "c2000000", "c3000000", "c4000000"]);
  await expect(page.locator("app-commit-graph-panel")).toContainText("4 selected");

  // a second shift-click re-stretches the SAME run instead of stacking on the old tail
  await rows(page).nth(3).click({ modifiers: ["Shift"] });
  expect(await selected(page)).toEqual(["c0000000", "c2000000", "c3000000"]);
});

test("shift-click walks a run upward from a plain click too", async ({ page }) => {
  await openGraph(page, "e2e-cg2");
  await rows(page).nth(3).click();
  await rows(page).nth(1).click({ modifiers: ["Shift"] });
  expect(await selected(page)).toEqual(["c3000000", "c1000000", "c2000000"]);
  await expect(page.locator("app-commit-graph-panel")).toContainText("3 selected");

  // a plain click collapses everything back to one row
  await rows(page).nth(0).click();
  expect(await selected(page)).toEqual(["c0000000"]);
});
