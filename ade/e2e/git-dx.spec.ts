import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the A4.1 dual-path git controls — v2: they live in the GIT ACTION
 * BAR docked under the diff (the right-panel Git tab is gone). Cost kill
 * switch OFF (src/app/cost/cost-flags.ts): the split buttons and AI variants
 * keep working, but NO token/$ chrome renders anywhere.
 *
 * Backend-free: the bar renders without a Tauri backend.
 */

const seedAgent = (id: string, name: string, commits = 0) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: ${commits}, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

const seedChanges = (id: string, n: number) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const work = bar.agentActions["work"];
  const files = Array.from({ length: ${n} }, (_, i) =>
    ({ path: "src/file-" + i + ".ts", add: 1, del: 0, state: "M" }));
  work["patch"](work["changesMap"], "${id}", { status: "ready", data: files });
})()`;

async function openActionBar(page: Page, id: string, name: string, commits = 0): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent(id, name, commits));
  await page.evaluate(ui(`.openAgent("${id}", "diff")`));
  await expect(page.locator("app-git-action-bar")).toBeVisible();
}

test("the action bar docks under the diff with dual-path commit / rebase / merge", async ({ page }) => {
  await openActionBar(page, "e2e-g1", "e2e-git");
  const bar = page.locator("app-git-action-bar");
  await expect(bar.locator("app-git-action-button")).toHaveCount(3);

  // real disabled states: clean tree → no commit/discard; no commits → no push/merge
  await expect(bar.getByRole("button", { name: /^Commit$/ })).toBeDisabled();
  await expect(bar.getByRole("button", { name: /^Push/ })).toBeDisabled();
  await expect(bar.getByRole("button", { name: /^Discard/ })).toBeDisabled();
  await expect(bar.getByPlaceholder("working tree clean")).toBeDisabled();
});

test("changes arm the bar: message input, stage toggle, commit primary", async ({ page }) => {
  await openActionBar(page, "e2e-g2", "e2e-armed");
  await page.evaluate(seedChanges("e2e-g2", 3));
  const bar = page.locator("app-git-action-bar");
  await expect(bar.getByPlaceholder(/commit message — 3 files/)).toBeEnabled();
  await expect(bar.getByRole("button", { name: /Staged all · 3/ })).toBeVisible();
  await expect(bar.getByRole("button", { name: /^Commit$/ })).toBeEnabled();
  // unstaging parks Commit
  await bar.getByRole("button", { name: /Staged all · 3/ }).click();
  await expect(bar.getByRole("button", { name: /^Commit$/ })).toBeDisabled();
  await expect(bar.getByRole("button", { name: /^Discard/ })).toBeEnabled();
});

test("merge dropdown offers the AI variant with NO cost chrome (kill switch off)", async ({ page }) => {
  // merge needs commits ahead — the bar's disabled states are real
  await openActionBar(page, "e2e-g3", "e2e-merge", 2);
  const merge = page.locator("app-git-action-bar app-git-action-button", { hasText: "Merge" });

  await merge.locator(".caret").click();
  // the dropdown is <kj-dropdown-menu-content>: portalled to the overlay
  // container, so it is NOT inside app-git-action-button any more
  const menu = page.locator("kj-dropdown-menu-content.menu:visible");
  await expect(menu).toBeVisible();
  await expect(menu).toContainText("AI path");
  await expect(menu).not.toContainText("spends tokens");

  // the variant row still works, but carries no token/$ estimate
  const row = menu.locator("kj-button.row", { hasText: "Merge with AI" });
  await expect(row).toBeVisible();
  await expect(row.locator(".row-est")).toHaveCount(0);

  await expect(menu).toContainText("native path is instant and deterministic");
  await expect(menu).not.toContainText("tokens");

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("action-bar buttons are content-sized; the message input takes the slack", async ({ page }) => {
  await openActionBar(page, "e2e-g5", "e2e-width");
  const bar = page.locator("app-git-action-bar");
  const input = await bar.locator("input.gab-msg").boundingBox();
  const commit = await bar
    .locator("app-git-action-button", { hasText: "Commit" })
    .boundingBox();
  expect(input && commit).toBeTruthy();
  // the split buttons must not stretch — the input owns the free space
  expect(commit!.width).toBeLessThan(input!.width);
  expect(commit!.width).toBeLessThan(260);
});

test("the dropdown is a portalled kj menu: fully inside the viewport, ABOVE a bottom-docked caret", async ({ page }) => {
  await openActionBar(page, "e2e-g6", "e2e-clip", 2);
  const merge = page.locator("app-git-action-bar app-git-action-button", { hasText: "Merge" });
  // the caret is a <kj-button> HOST (display:contents, no box of its own) —
  // measure the inner painted button instead
  const caret = await merge.locator(".caret .kj-button").boundingBox();
  await merge.locator(".caret").click();
  // kouji portals the menu content out of the button, to the overlay container
  const panel = page.locator("kj-dropdown-menu-content.menu:visible");
  await expect(panel).toBeVisible();
  // let the entry animation finish — boundingBox mid-transform lies
  await page.waitForTimeout(400);
  const box = await panel.boundingBox();
  const viewport = page.viewportSize()!;
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width + 1);
  // the bar sits at the bottom — the menu must open UP, not clamp over the caret
  if (caret!.y + caret!.height + box!.height > viewport.height - 8) {
    expect(box!.y + box!.height).toBeLessThanOrEqual(caret!.y);
  }
  // caret click toggles closed (the menu's own dismissal must not re-open it)
  await merge.locator(".caret").click();
  await expect(panel).toHaveCount(0);
});

test("rebase is aiOnly until A3.4: usable, with no inline estimate while costs are off", async ({ page }) => {
  await openActionBar(page, "e2e-g4", "e2e-rebase");
  const rebase = page.locator("app-git-action-bar app-git-action-button", { hasText: "Rebase onto" });
  await expect(rebase.locator("kj-button.main")).toBeVisible();
  await expect(rebase.locator(".est-inline")).toHaveCount(0);
});
