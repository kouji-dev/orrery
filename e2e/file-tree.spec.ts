import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the file-tree context menu (B1.1 CRUD): the root-scoped menu opens
 * from ANY right-click inside the panel — header, empty space, empty state —
 * not only on file/folder rows.
 *
 * Backend-free: the tree scan invoke rejects, so the panel shows its honest
 * "empty worktree" state — exactly the empty-space case the menu must serve.
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

async function openFilesTab(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-ft1", "e2e-tree"));
  await page.evaluate(ui(`.openAgent("e2e-ft1")`));
  await page.locator("app-right-panel").getByRole("tab", { name: "Files" }).click();
  await expect(page.locator("app-file-tree")).toBeVisible();
}

test("right-clicking empty panel space opens the root-scoped CRUD menu", async ({ page }) => {
  await openFilesTab(page);
  await page.locator("app-file-tree").click({ button: "right" });

  const menu = page.locator(".menu-panel");
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("button", { name: "New File…" })).toBeVisible();
  await expect(menu.getByRole("button", { name: "New Folder…" })).toBeVisible();
  // root scope (no node): per-node actions stay hidden
  await expect(menu.getByRole("button", { name: "Rename…" })).toHaveCount(0);
  await expect(menu.getByRole("button", { name: "Delete" })).toHaveCount(0);

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("sidebar agent rows show change counters from the dedicated totals map", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-ct1", "e2e-counts"));
  // the sidebar renders agents under their PROJECT group — seed the record
  await page.evaluate(`(() => {
    const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
    bar.projects["projectsStore"]["store"].upsert({
      id: "p-e2e", name: "e2e-proj", path: "C:/e2e", icon: "box", color: "#22d3ee",
      folderExists: true, hasGit: true, branch: "main",
    });
  })()`);
  // seed the totals map the way an init pass / full scan would
  await page.evaluate(`(() => {
    const work = window.ng.getComponent(document.querySelector("app-top-bar")).agentActions["work"];
    work["patch"](work["totalsMap"], "e2e-ct1", { add: 12, del: 4, files: 3 });
  })()`);
  const row = page.locator("app-agent-row", { hasText: "e2e-counts" });
  await expect(row).toContainText("+12");
  await expect(row).toContainText("−4");
});

test("sidebar: right-clicking empty projects space opens the panel menu", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.locator("app-sidebar .scroll-y").click({ button: "right" });
  const menu = page.locator("app-context-menu");
  await expect(menu.getByText("Add Project…")).toBeVisible();
  await expect(menu.getByText("Spawn Agent…")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu.getByText("Add Project…")).toHaveCount(0);
});

test("New File… from the empty-space menu targets the worktree root", async ({ page }) => {
  await openFilesTab(page);
  await page.locator("app-file-tree").click({ button: "right" });
  await page.locator(".menu-panel").getByRole("button", { name: "New File…" }).click();
  await expect(page.locator(".menu-panel")).toContainText("New file in worktree root");
  await expect(page.locator(".menu-input")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".menu-panel")).toHaveCount(0);
});
