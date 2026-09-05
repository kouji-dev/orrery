import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the versioned model catalog in the Spawn agent dialog.
 *
 * The catalog moved from flat id strings to per-model entries with their own
 * effort levels: Claude's aliases (fable/opus/sonnet) and pinned versions
 * (claude-opus-4-6…) each say which `--effort` levels they take — Haiku none,
 * the 4.6 generation no `xhigh` — so the Reasoning-effort tray must follow the
 * picked MODEL, not just the tool. That wiring only exists in the real kouji
 * listbox + tabs, hence Playwright rather than vitest.
 *
 * Also here: the Source-branch picker must present branches in the order the
 * backend hands them (pinned trunk names first, then by last use) — nothing in
 * the dialog may re-sort them alphabetically.
 */

const bar = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar"))${expr}`;

// Deliberately NOT alphabetical: the backend's picker order is pinned names
// first, then most-recently-used — a re-sort anywhere in the dialog would
// scramble it back to a/b/c.
const BRANCHES = ["main", "dev", "feat/yesterday", "feat/last-week", "a-alphabetically-first"];

const seed = `(() => {
  const b = window.ng.getComponent(document.querySelector("app-top-bar"));
  b.projects["projectsStore"]["store"].upsert({
    id: "7f3a91c4-2b5e-4d18-9a06-c1e8f4b7d302",
    name: "payments-service",
    path: "/home/me/code/payments-service",
    icon: "folder", color: "#6ea8fe",
    folderExists: true, hasGit: true,
    defaultBranch: "main",
    branches: ${JSON.stringify(BRANCHES)},
    head: "abc1234",
  });
})()`;

async function openSpawn(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seed);
  await page.evaluate(bar(`.ui.openSpawn(null)`));
  await page.waitForSelector("app-spawn-modal", { state: "attached" });
  await expect(page.getByRole("heading", { name: "Spawn agent" })).toBeVisible();
}

/** The pickers in template order: project, branch, ticket, model. */
const picker = (page: Page, i: number) =>
  page.locator("app-spawn-modal app-select").nth(i).locator(".kj-select-trigger");
const BRANCH = 1;
const MODEL = 3;

/** kouji portals the open listbox to the overlay container; match the OPEN one. */
const panel = (page: Page) =>
  page.locator('.kj-overlay-container .kj-select-content[data-state="open"]');

const modal = (page: Page, expr: string) =>
  page.evaluate(`window.ng.getComponent(document.querySelector("app-spawn-modal"))${expr}`);

const effortTabs = (page: Page) => page.locator("app-spawn-modal .spawn-seg .kj-tab");
const chosenEffort = (page: Page) => page.locator('app-spawn-modal .spawn-seg .kj-tab[aria-selected="true"]');

async function pickModel(page: Page, label: string): Promise<void> {
  await picker(page, MODEL).click();
  await panel(page).getByRole("option", { name: label, exact: true }).click();
}

test("Claude: the effort tray follows the picked model — Haiku hides it, Opus 4.6 drops xhigh", async ({ page }) => {
  await openSpawn(page);
  // Claude is the default tool; its default model (the fable alias) takes all
  // five Claude Code levels and pre-selects xhigh, Claude Code's own default
  await expect(picker(page, MODEL)).toHaveText(/Fable \(latest\)/);
  await expect(effortTabs(page)).toHaveText(["low", "medium", "high", "xhigh", "max"]);
  await expect(chosenEffort(page)).toHaveText("xhigh");
  expect(await modal(page, ".effort()")).toBe("xhigh");

  // Haiku takes no effort: the tray disappears and the record carries null
  await pickModel(page, "Haiku (latest)");
  await expect(page.locator("app-spawn-modal .spawn-seg")).toHaveCount(0);
  expect(await modal(page, ".effort()")).toBeNull();
  expect(await modal(page, ".model()")).toBe("haiku");

  // Opus 4.6 predates xhigh: four levels, and with nothing to keep the effort
  // lands on the model's default (high)
  await pickModel(page, "Opus 4.6");
  await expect(effortTabs(page)).toHaveText(["low", "medium", "high", "max"]);
  await expect(chosenEffort(page)).toHaveText("high");
  expect(await modal(page, ".model()")).toBe("claude-opus-4-6");

  // a level the next model also accepts is KEPT across the switch…
  await effortTabs(page).filter({ hasText: /^max$/ }).click();
  await pickModel(page, "Opus 5");
  await expect(chosenEffort(page)).toHaveText("max");
  expect(await modal(page, ".model()")).toBe("claude-opus-5");
  // …and one it doesn't falls back to that model's default
  await pickModel(page, "Opus 4.6");
  await effortTabs(page).filter({ hasText: /^max$/ }).click();
  await pickModel(page, "Fable 5.1");
  await expect(chosenEffort(page)).toHaveText("max");
  await effortTabs(page).filter({ hasText: /^xhigh$/ }).click();
  await pickModel(page, "Sonnet 4.6");
  await expect(chosenEffort(page)).toHaveText("high");
});

test("Model picker groups aliases and pinned versions, and forwards the exact --model id", async ({ page }) => {
  await openSpawn(page);
  await picker(page, MODEL).click();
  const list = panel(page);
  await expect(list).toBeVisible();
  // group headings ride along in the listbox
  await expect(list).toContainText("Latest");
  await expect(list).toContainText("Pinned versions");
  // human labels in the list, the exact CLI id on the record
  await list.getByRole("option", { name: "Opus 5", exact: true }).click();
  await expect(picker(page, MODEL)).toHaveText(/Opus 5/);
  expect(await modal(page, ".model()")).toBe("claude-opus-5");
});

test("Codex: Sol alone offers max and pre-selects xhigh; Luna pre-selects medium", async ({ page }) => {
  await openSpawn(page);
  await page.locator("app-spawn-modal .tool-tile", { hasText: "Codex" }).click();
  await expect(picker(page, MODEL)).toHaveText(/GPT-5.6 Sol/);
  await expect(effortTabs(page)).toHaveText(["low", "medium", "high", "xhigh", "max"]);
  await expect(chosenEffort(page)).toHaveText("xhigh");

  await pickModel(page, "GPT-5.6 Luna");
  await expect(effortTabs(page)).toHaveText(["low", "medium", "high", "xhigh"]);
  // xhigh is still offered on Luna, so the level is kept — the per-model
  // default (medium) only applies when nothing carries over…
  await expect(chosenEffort(page)).toHaveText("xhigh");
  // …which is the case for Sol's `max`: Luna has no max, so it lands on medium
  await pickModel(page, "GPT-5.6 Sol");
  await effortTabs(page).filter({ hasText: /^max$/ }).click();
  await pickModel(page, "GPT-5.6 Luna");
  await expect(chosenEffort(page)).toHaveText("medium");
});

test("Cursor and Gemini expose no effort tray, only a model list", async ({ page }) => {
  await openSpawn(page);
  for (const tool of ["Cursor", "Gemini"]) {
    await page.locator("app-spawn-modal .tool-tile", { hasText: tool }).click();
    await expect(page.locator("app-spawn-modal .spawn-seg")).toHaveCount(0);
    expect(await modal(page, ".effort()")).toBeNull();
  }
  await expect(picker(page, MODEL)).toHaveText(/Auto \(Gemini 3\)/);
});

test("Source branch keeps the backend's order: pinned trunk names first, then by last use", async ({ page }) => {
  await openSpawn(page);
  await expect(picker(page, BRANCH)).toHaveText(/main/);
  await picker(page, BRANCH).click();
  const options = panel(page).getByRole("option");
  await expect(options).toHaveCount(BRANCHES.length); // the listbox has rendered
  const names = await options.allTextContents();
  expect(names.map((n) => n.trim())).toEqual(BRANCHES);
});
