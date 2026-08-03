import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the B2 navigation surfaces (command registry + palette, Search
 * Everywhere, recent files, go-to-line) and the B3.1 find-in-files overlay.
 *
 * The browser build has no Tauri backend (every invoke rejects), so the specs
 * exercise the pure-frontend contract: overlays open from their keybindings,
 * the registry renders and filters, disabled commands flash instead of
 * running, and the find overlay opens with its full control row.
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

/** The shell renders after a startup loading screen — wait for it. */
async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

test("Ctrl+Shift+P opens the command palette; typing filters; Esc closes", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+Shift+P");
  const palette = page.locator("app-command-palette");
  // the custom-element host is a 0×0 inline box (content is fixed-positioned)
  // — assert on the input, which has a real box
  await expect(palette.locator("input")).toBeVisible();
  // registry renders FROM the command list (B2.2) — count chip present
  await expect(palette.locator(".chip")).toContainText("commands");

  await page.keyboard.type("theme");
  await expect(palette).toContainText("Theme"); // "Switch to … Theme"
  await expect(palette).not.toContainText("Spawn Agent"); // filtered out

  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
});

test("palette runs a command: Settings opens from the keyboard alone", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+Shift+P");
  await page.keyboard.type("settings");
  await page.keyboard.press("Enter");
  await expect(page.locator("app-command-palette")).toHaveCount(0);
  // the Settings modal is the command's effect
  await expect(page.locator("app-set-row").first()).toBeVisible();
});

test("double-Shift opens Search Everywhere with the Commands corpus", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Shift");
  await page.keyboard.press("Shift");
  const se = page.locator("app-search-everywhere");
  await expect(se.locator("input")).toBeVisible();

  await page.keyboard.type("spawn");
  await expect(se).toContainText("Spawn Agent");

  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});

test("topbar Search Everywhere button opens the overlay", async ({ page }) => {
  await ready(page);
  const btn = page.locator("app-top-bar .tb-search");
  await expect(btn).toBeVisible();
  // kbd chip shows the double-shift label (Windows-first: "Shift Shift")
  await expect(btn.locator(".kbd")).toContainText("Shift");
  await btn.click();
  const se = page.locator("app-search-everywhere");
  // host is a 0×0 inline box — assert the inner input, which has a real box
  await expect(se.locator("input")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});

test("Ctrl+K opens Search Everywhere (kbdAlt of double-Shift)", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+K");
  const se = page.locator("app-search-everywhere");
  await expect(se.locator("input")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});

test("Ctrl+E shows recent files (empty state before any file was opened)", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+E");
  const recent = page.locator("app-recent-files-overlay");
  await expect(recent.getByText("no files opened yet")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(recent).toHaveCount(0);
});

test("Ctrl+L without an open file flashes 'not available' instead of running", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+L");
  await expect(page.locator("app-goto-line-overlay")).toHaveCount(0);
  await expect(page.getByText("Go to Line… — not available here")).toBeVisible();
});

test("Ctrl+Shift+F opens find-in-files with scope + toggles once an agent exists", async ({ page }) => {
  await ready(page);
  await page.evaluate(seedAgent("e2e-f1", "e2e-finder"));
  await page.evaluate(ui(`.openAgent("e2e-f1")`));

  await page.keyboard.press("Control+Shift+F");
  const find = page.locator("app-find-in-files");
  await expect(find.locator("input")).toBeVisible();

  // control row: Find/Replace seg (Replace stubbed until B3.2), Aa/W/.* toggles, scope select
  await expect(find.getByRole("button", { name: "Replace" })).toBeDisabled();
  await expect(find.getByRole("button", { name: "Aa" })).toBeVisible();
  await expect(find.getByRole("button", { name: ".*" })).toBeVisible();
  await expect(find.locator("select")).toBeVisible();
  await expect(find).toContainText("type to search");

  // idle empty-state hint before a query
  await expect(find).toContainText("results stream in as files are scanned");

  await page.keyboard.press("Escape");
  await expect(find).toHaveCount(0);
});

test("Search Everywhere: Actions is the default tab; Files is lazy and grouped by project", async ({ page }) => {
  await ready(page);
  await page.evaluate(seedAgent("e2e-se1", "e2e-grouped"));
  await page.keyboard.press("Shift");
  await page.keyboard.press("Shift");
  const se = page.locator("app-search-everywhere");
  await expect(se.locator("input")).toBeVisible();

  // Actions first + default (design commands.jsx); no "All" tab anymore
  await expect(se.getByRole("button", { name: /^Actions/ })).toBeVisible();
  await expect(se.getByRole("button", { name: /^All/ })).toHaveCount(0);
  await expect(se).toContainText("Show All Commands"); // commands render with no query

  // Files is LAZY: blank until the first character is typed
  await se.getByRole("button", { name: /^Files/ }).click();
  await expect(se).toContainText("start typing to search files");

  // seed the LIVE component's file corpus directly (signal write — no
  // close/reopen dance, which raced the 380ms double-shift window under load)
  await page.evaluate(`window.ng.getComponent(document.querySelector("app-search-everywhere"))
    ["fileList"].set([{ agentId: "e2e-se1", projectId: "p-e2e", paths: ["src/alpha.ts", "src/beta.ts"] }])`);
  // clicking the tab moved focus off the input — fill targets it directly
  await se.locator("input").fill("alpha");

  // grouped result: sticky project header (no project record -> the honest
  // "Outside project" bucket) above the matching file row
  await expect(se.getByText("Outside project")).toBeVisible();
  await expect(se).toContainText("alpha.ts");
  await expect(se).not.toContainText("beta.ts"); // filtered out

  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});
