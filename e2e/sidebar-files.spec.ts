import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the v2 sidebar FILES SECTION — the repo tree moved out of the deleted
 * right panel. The root chip is the critical element: it names the worktree the
 * tree is rooted at, follows the active tab, and an explicit pick overrides it
 * visibly. Tree rows are QUIET: no A/M/D letters, a small modified dot only.
 *
 * Backend-free: tree/changes data is seeded straight into AgentWorkStore.
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-e2e", name: "e2e-proj", path: "C:/e2e", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main",
  });
})()`;

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "C:/wt/${name}", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Seed a ready tree + one modified file for a root key (agent id or proj:<id>). */
const seedTree = (key: string) => `(() => {
  const work = window.ng.getComponent(document.querySelector("app-top-bar")).agentActions["work"];
  work["patch"](work["treesMap"], "${key}", { status: "ready", data: [
    { name: "src", path: "src", isDir: true, ignored: false, children: [
      { name: "main.ts", path: "src/main.ts", isDir: false, ignored: false, children: null },
      { name: "app.ts", path: "src/app.ts", isDir: false, ignored: false, children: null },
    ] },
    { name: "readme.md", path: "readme.md", isDir: false, ignored: false, children: null },
  ]});
  work["patch"](work["changesMap"], "${key}", { status: "ready", data: [
    { path: "src/main.ts", state: "M", add: 3, del: 1 },
  ]});
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent("e2e-sf1", "alpha"));
  await page.evaluate(seedAgent("e2e-sf2", "beta"));
}

test("root chip follows the active agent tab", async ({ page }) => {
  await boot(page);
  await page.evaluate(ui(`.openAgent("e2e-sf1")`));
  const chip = page.locator("app-sidebar-files button[title*='Worktree root']");
  await expect(chip).toContainText("alpha");
  await page.evaluate(ui(`.openAgent("e2e-sf2")`));
  await expect(chip).toContainText("beta");
});

test("explicit root pick overrides follow — scope visible, never implicit", async ({ page }) => {
  await boot(page);
  await page.evaluate(ui(`.openAgent("e2e-sf1")`));
  const files = page.locator("app-sidebar-files");
  const chip = files.locator("button[title*='Worktree root']");
  await chip.click();
  // dropdown groups by project: a main (repo root) row + each agent worktree
  await expect(files.getByText("repo root")).toBeVisible();
  await files.getByRole("button", { name: /beta/ }).click();
  await expect(chip).toContainText("beta");
  // the active tab is still alpha's — the chip shows the OVERRIDE, visibly
  await page.evaluate(ui(`.openAgent("e2e-sf2")`));
  await expect(chip).toContainText("beta");
});

test("picking main roots the tree at the repo root", async ({ page }) => {
  await boot(page);
  await page.evaluate(ui(`.openAgent("e2e-sf1")`));
  await page.evaluate(seedTree("proj:p-e2e"));
  const files = page.locator("app-sidebar-files");
  await files.locator("button[title*='Worktree root']").click();
  await files.getByRole("button", { name: /main/ }).first().click();
  await expect(files.locator("button[title*='Worktree root']")).toContainText("main");
  await expect(files.locator("app-sidebar-file-tree")).toContainText("readme.md");
});

test("quiet tree: modified files get a dot, never a letter badge", async ({ page }) => {
  await boot(page);
  await page.evaluate(ui(`.openAgent("e2e-sf1")`));
  await page.evaluate(seedTree("e2e-sf1"));
  const tree = page.locator("app-sidebar-file-tree");
  // expand src/
  await tree.getByText("src", { exact: true }).click();
  const modified = tree.locator("div", { hasText: "main.ts" }).last();
  await expect(modified.locator("span[title*='modified']")).toBeVisible();
  // no A/M/D letter badges anywhere in the quiet tree
  await expect(tree.getByText("M", { exact: true })).toHaveCount(0);
});

test("section collapses to its header strip and back", async ({ page }) => {
  await boot(page);
  await page.evaluate(ui(`.openAgent("e2e-sf1")`));
  await page.evaluate(seedTree("e2e-sf1"));
  const files = page.locator("app-sidebar-files");
  await expect(files.locator("app-sidebar-file-tree")).toBeVisible();
  await files.locator("button[title='Collapse files']").click();
  await expect(files.locator("app-sidebar-file-tree")).toHaveCount(0);
  await expect(files.locator("button[title*='Worktree root']")).toBeVisible(); // header stays
  await files.locator("button[title='Expand files']").click();
  await expect(files.locator("app-sidebar-file-tree")).toBeVisible();
});

test("compact rail shows a files icon that expands the sidebar", async ({ page }) => {
  await boot(page);
  await page.evaluate(ui(`.toggleSidebarCompact()`));
  await expect(page.locator("app-compact-rail")).toBeVisible();
  await page.locator("app-compact-rail button[title='Files — expand sidebar']").click();
  await expect(page.locator("app-sidebar")).toBeVisible();
  await expect(page.locator("app-sidebar-files")).toBeVisible();
});
