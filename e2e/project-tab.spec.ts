import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the v2 PROJECT TAB — "an agent is just a worktree with a process
 * attached": the same pane tree rooted at the project's MAIN worktree, a plain
 * shell where an agent tab has the tool PTY, the diff view showing main's
 * working changes, and the sidebar files root chip following the tab to main.
 *
 * Backend-free: invokes reject, so run-state stays idle; structure is what's
 * asserted (the shell PTY itself needs the Tauri backend).
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-e2e", name: "e2e-proj", path: "C:/e2e", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main",
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

const seedChanges = (id: string) => `(() => {
  const work = window.ng.getComponent(document.querySelector("app-top-bar")).agentActions["work"];
  work["patch"](work["changesMap"], "${id}", { status: "ready", data: [
    { path: "src/app.ts", add: 4, del: 1, state: "M" },
  ]});
})()`;

async function openProjectTab(page: Page, pane?: string): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(ui(`.openProject("p-e2e"${pane ? `, "${pane}"` : ""})`));
}

test("the project tab renders with a project chip and the main branch chip", async ({ page }) => {
  await openProjectTab(page);
  const strip = page.locator("app-top-bar .tab-strip");
  await expect(strip).toContainText("e2e-proj");
  await expect(strip).toContainText("main");
  // the workspace pane tree hosts the tab (same component as agent tabs)
  await expect(page.locator("app-pane-manager")).toBeVisible();
});

test("the terminal pane is a plain shell session on the main worktree", async ({ page }) => {
  await openProjectTab(page, "terminal");
  const pane = page.locator("app-pane-manager .pane-leaf");
  await expect(pane).toContainText("e2e-proj"); // pseudo-agent named after the project
  await expect(pane.locator("app-terminal")).toBeVisible();
  await expect(pane.locator("app-terminal")).toContainText("C:/e2e"); // session header = repo root
  // play button reads as a shell control, not an agent run control
  await expect(pane.locator("button[title='Open shell']")).toBeVisible();
});

test("the diff pane shows main's working changes with the action bar (no rebase/merge)", async ({ page }) => {
  await openProjectTab(page, "diff");
  await page.evaluate(seedChanges("p-e2e"));
  const pane = page.locator("app-pane-manager .pane-leaf");
  await expect(pane.locator("app-diff-view")).toBeVisible();
  await expect(pane).toContainText("app.ts");
  const bar = pane.locator("app-git-action-bar");
  await expect(bar).toBeVisible();
  await expect(bar.getByRole("button", { name: /^Commit$/ })).toBeVisible();
  // branch-integration verbs stay hidden — a project tab IS the base branch
  await expect(bar.getByRole("button", { name: /Rebase onto/ })).toHaveCount(0);
  await expect(bar.getByRole("button", { name: /Merge/ })).toHaveCount(0);
});

test("the sidebar files root chip follows the project tab to main", async ({ page }) => {
  await openProjectTab(page);
  const chip = page.locator("app-sidebar-files button[title*='Worktree root']");
  await expect(chip).toContainText("main");
});

test("re-opening the same project reuses its tab", async ({ page }) => {
  await openProjectTab(page);
  await page.evaluate(ui(`.selectTab("orchestrator")`));
  await page.evaluate(ui(`.openProject("p-e2e")`));
  const projTabs = page.locator("app-top-bar .tab-strip > div", { hasText: "e2e-proj" });
  await expect(projTabs).toHaveCount(1);
});

test("the project context menu offers Open project workspace", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.locator("app-project-group", { hasText: "e2e-proj" }).first().click({ button: "right" });
  const menu = page.locator("app-context-menu");
  await expect(menu.getByText("Open project workspace")).toBeVisible();
  await menu.getByText("Open project workspace").click();
  await expect(page.locator("app-top-bar .tab-strip")).toContainText("e2e-proj");
});
