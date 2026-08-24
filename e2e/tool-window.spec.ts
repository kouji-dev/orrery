import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the bottom tool window (IntelliJ-style dock) and its git panels:
 * the dock opens from the registry keybindings/palette (closed by default),
 * the tab strip renders git panels first with the accent underline, the
 * scope selectors follow the seeded project/worktree, and the LIVE branch /
 * local-history panels (A3.2 / B4.4) render honest empty states when the
 * backend is absent.
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
  await expect(dock.getByRole("tab", { name: "Git Graph" })).toBeVisible();
  await expect(dock.getByRole("tab", { name: "Branches" })).toBeVisible();
  await expect(dock.getByRole("tab", { name: "Local History" })).toBeVisible();
  // the strip is <kj-tabs> now — the active tab is the aria-selected one, and
  // kouji draws the underline from it (no hand-rolled .tab-ind span)
  await expect(dock.getByRole("tab", { name: "Git Graph" })).toHaveAttribute("aria-selected", "true");
  // no agents yet → the graph panel asks for a worktree scope (no fake rows)
  await expect(dock.locator("app-commit-graph-panel")).toContainText("select a worktree");

  // same binding toggles the dock away (IntelliJ behavior)
  await page.keyboard.press("Control+Shift+G");
  await expect(dock).toHaveCount(0);
});

test("the palette lists and runs the tool-window commands", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+Shift+P");
  // visible is not enough: the palette focuses its input in a microtask after
  // render, and a keystroke that lands before that goes to the document —
  // where the app's own shortcuts eat it and can close the palette outright
  await expect(page.locator("app-command-palette input")).toBeFocused();
  await page.keyboard.type("commit graph");
  await expect(page.locator("app-command-palette")).toContainText("Show Commit Graph");
  // Enter runs whatever row is HIGHLIGHTED — wait for the palette to settle on
  // one, or a loaded machine can press Enter between the query landing and the
  // list re-rendering under it.
  await expect(page.locator(".kj-command-item[data-active]")).toHaveCount(1);
  await page.keyboard.press("Enter");
  // Enter must first CLOSE the palette (the command ran); only then does the
  // dock mount. Splitting the two makes a failure say which half broke.
  await expect(page.locator("app-command-palette")).toHaveCount(0);
  await expect(page.locator("app-tool-window")).toHaveCount(1);
  await expect(page.locator("app-commit-graph-panel")).toHaveCount(1);
});

test("graph panel: real scope, filter chrome, and the B4.1 path filter disabled", async ({ page }) => {
  await openSeeded(page);
  const panel = page.locator("app-commit-graph-panel");

  // scoped to the seeded worktree: filter row renders; commits fetch rejects
  // (no backend) → honest empty state, never fabricated rows
  await expect(panel.locator(`input[placeholder="Filter by message or sha…"]`)).toBeVisible();
  await expect(panel).toContainText("no commits on this branch yet");
  await expect(panel).toContainText("shift-click two commits");

  // the B4.1 note lives on the <kj-input>'s wrapper, not on the native input
  const path = panel.locator(`input[placeholder="path…"]`);
  await expect(path).toBeDisabled();
  await expect(panel.getByTitle(/B4\.1/)).toBeVisible();

  // the scope cluster shows the seeded project + worktree — <app-select> is a
  // kouji <kj-select> now, so the trigger LABEL is what's assertable (not a value)
  const dock = page.locator("app-tool-window");
  await expect(dock.getByTitle("Project the panels read from")).toContainText("e2e-proj");
  await expect(dock.getByTitle("Worktree the panel reads from")).toContainText("e2e-tw");
});

test("branches panel: live A3.2 chrome with honest no-backend states", async ({ page }) => {
  await openSeeded(page);
  await page.locator("app-tool-window").getByRole("tab", { name: "Branches" }).click();
  const panel = page.locator("app-branches-panel");

  // REAL data: detected project branches + the seeded worktree branch
  // (backend absent → the native detail load fails, names fall back)
  await expect(panel).toContainText("Branches · e2e-proj");
  await expect(panel).toContainText("main");
  await expect(panel).toContainText("dev");
  await expect(panel).toContainText("agent/e2e-tw");
  // the scoped worktree's branch is HEAD
  await expect(panel.locator("kj-badge", { hasText: "HEAD" })).toHaveCount(1);

  // remotes column: backend rejected → honest empty state, no fake remotes
  await expect(panel).toContainText("Remotes");
  await expect(panel).toContainText("no remotes configured");

  // live ops: New branch stays disabled until a name is typed, then enables
  const newBranch = panel.getByRole("button", { name: "New branch" }).first();
  await expect(newBranch).toBeDisabled();
  await panel.locator(`input[placeholder="new branch name…"]`).fill("feat-e2e");
  await expect(newBranch).toBeEnabled();

  // row ops are enabled buttons now (invokes reject backend-side — no fake ok)
  const checkout = panel.getByRole("button", { name: "Checkout" }).first();
  await expect(checkout).toBeEnabled();
  // Delete opens a <kj-confirm-popup> — its content is portalled out of the
  // panel, so the confirm chrome is asserted at page scope
  await panel.getByRole("button", { name: "Delete" }).first().click();
  // every row owns a (closed) popup, so match the OPEN one, not the first
  await expect(page.locator("kj-confirm-popup-message:visible")).toHaveText(/delete .+\?/);
  await page.getByRole("button", { name: "Cancel" }).first().click();
});

test("local history panel: live B4.4 chrome with an honest empty timeline", async ({ page }) => {
  await openSeeded(page);
  await page.locator("app-tool-window").getByRole("tab", { name: "Local History" }).click();
  const panel = page.locator("app-local-history-panel");

  // backend absent → the timeline load fails to an honest empty state
  await expect(panel).toContainText("Snapshots");
  await expect(panel).toContainText("no snapshots yet");
  await expect(panel).toContainText("captured automatically");

  // revert needs a selected snapshot — disabled with none
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
