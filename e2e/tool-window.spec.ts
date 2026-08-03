import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the bottom tool window (IntelliJ-style dock) and its git panels:
 * the dock opens from the registry keybindings/palette (closed by default),
 * the tab strip renders git panels first with the accent underline, the
 * scope selectors follow the seeded project/worktree, and not-yet-native ops
 * (A3.2 branch/remote mutations, B4.4 local-history snapshots) render
 * design-faithful but DISABLED with explanatory tooltips.
 *
 * Backend-free: every invoke rejects, so the specs assert chrome, real
 * seeded-store data, and disabled/empty states — never fake git data.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Seed a project through the open dock's ProjectActionsService handle. */
const seedProject = `(() => {
  const tw = window.ng.getComponent(document.querySelector("app-tool-window"));
  tw.projects["projectsStore"]["store"].upsert({
    id: "p-e2e", name: "e2e-proj", path: "", icon: "box", color: "#a855f7",
    folderExists: true, hasGit: true, branch: "main", branches: ["main", "dev"],
  });
})()`;

async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

/** Open the dock (any panel) and seed project + agent into its scope. */
async function openSeeded(page: Page): Promise<void> {
  await ready(page);
  await page.evaluate(seedAgent("e2e-tw1", "e2e-tw"));
  await page.keyboard.press("Control+Shift+G");
  await expect(page.locator("app-tool-window")).toHaveCount(1);
  await page.evaluate(seedProject);
}

test("Ctrl+Shift+G toggles the dock on Git Graph; closed by default", async ({ page }) => {
  await ready(page);
  // closed by default
  await expect(page.locator("app-tool-window")).toHaveCount(0);

  await page.keyboard.press("Control+Shift+G");
  const dock = page.locator("app-tool-window");
  await expect(dock).toHaveCount(1);

  // git panels first in the tab strip
  await expect(dock.getByRole("button", { name: "Git Graph" })).toBeVisible();
  await expect(dock.getByRole("button", { name: "Branches" })).toBeVisible();
  await expect(dock.getByRole("button", { name: "Local History" })).toBeVisible();
  // the active tab carries the accent underline
  await expect(dock.locator("button.tab", { hasText: "Git Graph" }).locator(".tab-line")).toHaveCount(1);
  // no agents yet → the graph panel asks for a worktree scope (no fake rows)
  await expect(dock.locator("app-commit-graph-panel")).toContainText("select a worktree");

  // same binding toggles the dock away (IntelliJ behavior)
  await page.keyboard.press("Control+Shift+G");
  await expect(dock).toHaveCount(0);
});

test("the palette lists and runs the tool-window commands", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+Shift+P");
  await page.keyboard.type("commit graph");
  await expect(page.locator("app-command-palette")).toContainText("Show Commit Graph");
  await page.keyboard.press("Enter");
  await expect(page.locator("app-tool-window")).toHaveCount(1);
  await expect(page.locator("app-commit-graph-panel")).toHaveCount(1);
});

test("graph panel: real scope, filter chrome, and the B4.1 path filter disabled", async ({ page }) => {
  await openSeeded(page);
  const panel = page.locator("app-commit-graph-panel");

  // scoped to the seeded worktree: filter row renders; commits fetch rejects
  // (no backend) → honest empty state, never fabricated rows
  await expect(panel.getByPlaceholder("Filter by message or sha…")).toBeVisible();
  await expect(panel).toContainText("no commits on this branch yet");
  await expect(panel).toContainText("shift-click two commits");

  const path = panel.getByPlaceholder("path…");
  await expect(path).toBeDisabled();
  await expect(path).toHaveAttribute("title", /B4\.1/);

  // the scope cluster shows the seeded project + worktree
  const dock = page.locator("app-tool-window");
  await expect(dock.locator("select").first()).toHaveValue("p-e2e");
  await expect(dock.getByTitle("Worktree the panel reads from")).toHaveValue("e2e-tw1");
});

test("branches panel: real branch list; every A3.2 op disabled with a tooltip", async ({ page }) => {
  await openSeeded(page);
  await page.locator("app-tool-window").getByRole("button", { name: "Branches" }).click();
  const panel = page.locator("app-branches-panel");

  // REAL data: detected project branches + the seeded worktree branch
  await expect(panel).toContainText("Branches · e2e-proj");
  await expect(panel).toContainText("main");
  await expect(panel).toContainText("dev");
  await expect(panel).toContainText("agent/e2e-tw");
  // the scoped worktree's branch is HEAD
  await expect(panel.locator(".chip", { hasText: "HEAD" })).toHaveCount(1);

  // remotes column: no fake remotes — an honest A3.2 note + disabled Add
  await expect(panel).toContainText("Remotes");
  await expect(panel).toContainText("no remotes to show yet");
  const add = panel.getByRole("button", { name: "Add" });
  await expect(add).toBeDisabled();
  await expect(add).toHaveAttribute("title", /A3\.2/);

  // header actions (Fetch / Pull / New branch) are disabled dual-path buttons
  for (const name of ["Fetch", "Pull", "New branch"]) {
    const btn = panel.locator("app-git-action-button", { hasText: name }).first();
    await expect(btn.locator("button").first()).toBeDisabled();
  }
  // row ops (Checkout / Merge in / Delete…) all disabled with the A3.2 tooltip
  const checkout = panel.locator("button.br-op", { hasText: "Checkout" }).first();
  await expect(checkout).toBeDisabled();
  await expect(checkout).toHaveAttribute("title", /A3\.2/);
});

test("local history panel: honest B4.4 empty state and disabled actions", async ({ page }) => {
  await openSeeded(page);
  await page.locator("app-tool-window").getByRole("button", { name: "Local History" }).click();
  const panel = page.locator("app-local-history-panel");

  await expect(panel).toContainText("Snapshots");
  await expect(panel).toContainText("no snapshots yet");
  await expect(panel).toContainText("B4.4");

  const showDiff = panel.getByRole("button", { name: "Show diff" });
  await expect(showDiff).toBeDisabled();
  await expect(showDiff).toHaveAttribute("title", /B4\.4/);
  await expect(panel.getByRole("button", { name: "Revert to this point" })).toBeDisabled();
});

test("the close affordance hides the dock", async ({ page }) => {
  await openSeeded(page);
  await page.locator("app-tool-window").getByTitle("Hide tool window").click();
  await expect(page.locator("app-tool-window")).toHaveCount(0);
});

test("status-bar Git Graph item toggles the dock", async ({ page }) => {
  await ready(page);
  const bar = page.locator("app-status-bar");
  // one dock trigger in the footer: Git Graph (branch icon); the other panels
  // stay a tab-switch away once the dock is open
  const btn = bar.getByRole("button", { name: "Git Graph" });
  await expect(btn).toBeVisible();
  await expect(bar.getByRole("button", { name: "Branches" })).toHaveCount(0);
  await btn.click();
  const dock = page.locator("app-tool-window");
  await expect(dock).toHaveCount(1);
  // second press = IntelliJ toggle semantics — hides the dock
  await btn.click();
  await expect(dock).toHaveCount(0);
});
