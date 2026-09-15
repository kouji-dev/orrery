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
  // kouji portals the panel into the shared overlay container, so nothing it
  // renders is a DOM descendant of <app-command-palette> — the panel's own
  // root carries the .kj-command-palette class
  const palette = page.locator(".kj-command-palette__shell");
  const paletteInput = page.locator(".kj-command-palette__input");
  await expect(paletteInput).toBeVisible();
  await expect(paletteInput).toBeFocused();
  // registry renders FROM the command list (B2.2) — footer count present
  await expect(page.locator(".orr-palette-count")).toContainText("commands");

  await page.keyboard.type("theme");
  await expect(palette).toContainText("Theme"); // "Switch to … Theme"
  await expect(palette).not.toContainText("Spawn Agent"); // filtered out

  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
});

test("palette runs a command: Settings opens from the keyboard alone", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+Shift+P");
  // visible is not enough: the palette focuses its input in a microtask after
  // render, and a keystroke that lands before that goes to the document —
  // where the app's own shortcuts eat it and can close the palette outright
  await expect(page.locator(".kj-command-palette__input")).toBeFocused();
  await page.keyboard.type("settings");
  // Enter runs whatever row is HIGHLIGHTED — wait for the palette to settle on
  // one, or a loaded machine can press Enter between the query landing and the
  // list re-rendering under it.
  await expect(page.locator(".kj-command-item[data-active]")).toHaveCount(1);
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
  // kbd chip advertises the PRIMARY binding — Ctrl+K (double-Shift stays as alt)
  // the chip is kouji's <kj-kbd> since the shared-primitive migration
  await expect(btn.locator("kj-kbd").first()).toContainText("Ctrl+K");
  await btn.click();
  const se = page.locator("app-search-everywhere");
  // host is a 0×0 inline box — assert the inner input, which has a real box
  await expect(se.locator("input")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});

test("Ctrl+K opens Search Everywhere (the primary binding)", async ({ page }) => {
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
  // portalled panel again (see above)
  await expect(page.getByText("no files opened yet")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(recent).toHaveCount(0);
});

/** Computed background/border a probe element gets from Orrery's tokens. */
const tokens = `(() => {
  const p = document.createElement("div");
  p.style.background = "var(--panel)";
  p.style.borderColor = "var(--hair)";
  document.body.appendChild(p);
  const cs = getComputedStyle(p);
  const v = { panel: cs.backgroundColor, hair: cs.borderTopColor };
  p.remove();
  return v;
})()`;

const seedRecents = (rows: { agentId: string; path: string }[]) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const svc = bar.commands["recents"];
  svc.entries.set(${JSON.stringify(rows.map((r) => ({ ...r, at: Date.now() })))});
})()`;

test("the recent-files palette wears the app surface, not kouji's default", async ({ page }) => {
  await ready(page);
  await page.keyboard.press("Control+E");
  const dialog = page.locator(".kj-command-palette__dialog");
  await expect(dialog).toBeVisible();

  // the panel is portalled to body level, so the skin has to reach it there —
  // when it does not, kouji's own (light) surface shows through instead
  const want = await page.evaluate(tokens);
  const got = await dialog.evaluate((el) => {
    const cs = getComputedStyle(el);
    return { bg: cs.backgroundColor, border: cs.borderTopColor, radius: cs.borderTopLeftRadius, shadow: cs.boxShadow };
  });
  expect(got.bg).toBe(want.panel);
  expect(got.border).toBe(want.hair);
  expect(parseFloat(got.radius)).toBeGreaterThan(0);
  expect(got.shadow).not.toBe("none");
  // …and the narrow variant still applies to the portalled dialog
  const width = await dialog.evaluate((el) => el.getBoundingClientRect().width);
  expect(width).toBeLessThan(560);
});

test("the recent-files footer says how many are shown, and of how many", async ({ page }) => {
  await ready(page);
  await page.evaluate(seedAgent("cn-r1", "alpha"));
  await page.evaluate(seedRecents([
    { agentId: "cn-r1", path: "src/alpha.ts" },
    { agentId: "cn-r1", path: "src/beta.ts" },
    { agentId: "cn-r1", path: "src/gamma.ts" },
  ]));
  await page.keyboard.press("Control+E");
  const count = page.locator(".orr-palette-count");
  await expect(count).toHaveText("3 recent files");

  // typing narrows: the count says what survived AND what it was drawn from
  await page.locator(".kj-command-palette__input").fill("alph");
  await expect(count).toHaveText("1 of 3 recent files");

  // a query with no match names itself — not the same emptiness as "none yet"
  await page.locator(".kj-command-palette__input").fill("zzzz");
  await expect(count).toHaveText("0 of 3 recent files");
  await expect(page.getByText(/no recent file matches/)).toBeVisible();
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

  // control row: live Find/Replace seg (B3.2), Aa/W/.* toggles, scope select
  const replaceTab = find.getByRole("tab", { name: "Replace" });
  await expect(replaceTab).toBeEnabled();
  await expect(find.getByRole("button", { name: "Aa" })).toBeVisible();
  await expect(find.getByRole("button", { name: ".*" })).toBeVisible();
  // the lone scope select is an <app-scope-bar> now: project + worktree + kind
  await expect(find.locator("app-scope-bar")).toBeVisible();
  // worktree + kind always; the project select only once a project exists, and
  // this spec seeds a bare agent — so assert the floor, not an exact shape
  expect(await find.locator("app-scope-bar kj-select").count()).toBeGreaterThanOrEqual(2);
  await expect(find).toContainText("type to search");

  // switching to Replace reveals the replacement row + apply button
  await replaceTab.click();
  await expect(find.locator('input[placeholder^="Replacement"]')).toBeVisible();
  await expect(find.getByRole("button", { name: /^Replace \d/ })).toBeDisabled();
  await find.getByRole("tab", { name: "Find", exact: true }).click();
  await expect(find.locator('input[placeholder^="Replacement"]')).toHaveCount(0);

  // idle empty-state hint before a query
  await expect(find).toContainText("results stream in as files are scanned");

  await page.keyboard.press("Escape");
  await expect(find).toHaveCount(0);
});

test("Search Everywhere: Actions default; Files is lazy, grouped PER WORKTREE with duplicates", async ({ page }) => {
  await ready(page);
  await page.evaluate(seedAgent("e2e-se1", "e2e-grouped"));
  await page.evaluate(seedAgent("e2e-se2", "e2e-other"));
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

  // seed the LIVE component's corpora directly (signal write — no close/reopen
  // dance, which raced the 380ms double-shift window under load): the SAME
  // path exists in two worktrees of one project
  await page.evaluate(`window.ng.getComponent(document.querySelector("app-search-everywhere"))
    ["fileList"].set([
      { agentId: "e2e-se1", projectId: "p-e2e", paths: ["src/alpha.ts", "src/beta.ts"] },
      { agentId: "e2e-se2", projectId: "p-e2e", paths: ["src/alpha.ts"] },
    ])`);
  // clicking the tab moved focus off the input — fill targets it directly
  await se.locator("input").fill("alpha");

  // per-worktree groups: one header per worktree (project · agent), and the
  // shared path appears once under EACH — no dedup across worktrees
  await expect(se.getByText("Outside project · e2e-grouped")).toBeVisible();
  await expect(se.getByText("Outside project · e2e-other")).toBeVisible();
  await expect(se.getByText("alpha.ts")).toHaveCount(2);
  await expect(se).not.toContainText("beta.ts"); // filtered out

  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});
