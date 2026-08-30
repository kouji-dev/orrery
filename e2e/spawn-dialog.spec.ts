import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the Spawn agent dialog's controls.
 *
 * Four regressions land here, and each needs the REAL control running — a
 * kouji listbox portalled to the overlay container, styled by the real
 * stylesheet. None of it is reachable from vitest, where the modal's signal
 * inputs are inert under JIT:
 *
 *   1. Project printed its raw id (a uuid) instead of the project name.
 *   2. Source branch grew to the whole viewport instead of scrolling inside
 *      its own panel.
 *   3. Ticket was still a native <select> while its neighbours were kouji.
 *   4. The Agent picker rendered four button ROWS, not four square tiles.
 */

const bar = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar"))${expr}`;

/** A project with a long branch list — the case that overflowed the listbox. */
const BRANCHES = Array.from({ length: 40 }, (_, i) => `feat/branch-${i + 1}`);

const seed = `(() => {
  const b = window.ng.getComponent(document.querySelector("app-top-bar"));
  b.projects["projectsStore"]["store"].upsert({
    id: "7f3a91c4-2b5e-4d18-9a06-c1e8f4b7d302",
    name: "payments-service",
    path: "/home/me/code/payments-service",
    icon: "folder", color: "#6ea8fe",
    folderExists: true, hasGit: true,
    defaultBranch: "main",
    branches: ${JSON.stringify(["main", ...BRANCHES])},
    head: "abc1234",
  });
  b.tickets["store"].upsert({
    id: "tk-1", title: "Fix the login bug", notes: "", status: "todo",
    projectId: "7f3a91c4-2b5e-4d18-9a06-c1e8f4b7d302", agentId: null,
  });
  b.tickets["store"].upsert({
    id: "tk-2", title: "Refactor auth module", notes: "", status: "inprogress",
    projectId: "7f3a91c4-2b5e-4d18-9a06-c1e8f4b7d302", agentId: null,
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

/** The four pickers in template order: project, branch, ticket, model. */
const picker = (page: Page, i: number) =>
  page.locator("app-spawn-modal app-select").nth(i).locator(".kj-select-trigger");

/** kouji portals the open listbox out of the modal, to the overlay container.
 *  Every picker keeps a panel mounted there, so match only the OPEN one. */
const panel = (page: Page) =>
  page.locator('.kj-overlay-container .kj-select-content[data-state="open"]');

test("Project shows the project NAME, not its id", async ({ page }) => {
  await openSpawn(page);
  const trigger = picker(page, 0);
  await expect(trigger).toHaveText(/payments-service/);
  await expect(trigger).not.toHaveText(/7f3a91c4/);

  // and the same holds after picking from the list, not just on first paint
  await trigger.click();
  await panel(page).getByRole("option", { name: "payments-service" }).click();
  await expect(trigger).toHaveText(/payments-service/);
});

test("Source branch scrolls inside its panel instead of filling the viewport", async ({ page }) => {
  await openSpawn(page);
  await picker(page, 1).click();
  const list = panel(page);
  await expect(list).toBeVisible();

  const box = await list.evaluate((el) => ({
    max: getComputedStyle(el).maxHeight,
    client: el.clientHeight,
    scroll: el.scrollHeight,
    viewport: window.innerHeight,
  }));
  // a real cap, not `100%` of the overlay container
  expect(box.max).not.toBe("100%");
  expect(parseFloat(box.max)).toBeGreaterThan(0);
  // 41 branches do not fit in it — the panel scrolls, the page does not grow
  expect(box.scroll).toBeGreaterThan(box.client);
  expect(box.client).toBeLessThan(box.viewport / 2);
});

test("Ticket is the same kouji picker as Model, with its status groups intact", async ({ page }) => {
  await openSpawn(page);
  // the native <select> it used to be is gone
  await expect(page.locator("app-spawn-modal select")).toHaveCount(0);

  const trigger = picker(page, 2);
  await expect(trigger).toHaveText(/None — start from scratch/);
  await trigger.click();

  const list = panel(page);
  await expect(list.getByText("To do", { exact: true })).toBeVisible();
  await expect(list.getByText("In progress", { exact: true })).toBeVisible();
  // the group headings are labels, not selectable rows
  await expect(list.getByRole("option")).toHaveCount(3);

  await list.getByRole("option", { name: "Fix the login bug" }).click();
  await expect(trigger).toHaveText(/Fix the login bug/);
  // linking a ticket prefills the worktree name
  await expect(page.locator(`app-spawn-modal input[placeholder="e.g. fix-login-bug"]`)).toHaveValue(
    "fix-the-login-bug",
  );
});

test("the header opens with the shared tinted icon square, and no worktree chip", async ({ page }) => {
  await openSpawn(page);
  const head = page.locator("app-spawn-modal .pane-head");
  // the same .head-icon square Add project uses — a bare glyph read as a
  // different class of dialog
  await expect(head.locator(".head-icon")).toBeVisible();
  await expect(head.locator(".head-icon app-icon")).toBeVisible();
  // the square is actually PAINTED — a .head-icon that resolved no accent
  // would pass the presence check above while still looking like a bare glyph
  const painted = await head.locator(".head-icon").evaluate((el) => {
    const s = getComputedStyle(el);
    return { bg: s.backgroundColor, border: parseFloat(s.borderTopWidth) };
  });
  expect(painted.bg).not.toBe("rgba(0, 0, 0, 0)");
  expect(painted.border).toBeGreaterThan(0);
  await expect(head.locator("kj-badge")).toHaveCount(0);
  await expect(page.locator("app-spawn-modal")).not.toContainText("new git worktree + branch");
});

test("the Initial prompt types at the same size as the Name field", async ({ page }) => {
  await openSpawn(page);
  const size = (sel: string) =>
    page.locator(sel).evaluate((el) => getComputedStyle(el).fontSize);
  const prompt = await size("app-spawn-modal .spawn-textarea textarea");
  expect(prompt).toBe(await size(`app-spawn-modal input[placeholder="e.g. fix-login-bug"]`));
  // …and both sit on the app's ramp, not on kouji's rem scale. ::placeholder
  // only recolours in kouji, so it inherits this size rather than needing a
  // check of its own.
  const baseline = await page.evaluate(
    `getComputedStyle(document.documentElement).getPropertyValue("--fs-body").trim()`,
  );
  expect(parseFloat(prompt)).toBeCloseTo(parseFloat(baseline as string), 1);
});

test("Agent picker renders four equal tiles, badge stacked over the name", async ({ page }) => {
  await openSpawn(page);
  const tiles = page.locator("app-spawn-modal .spawn-tools .kj-button");
  await expect(tiles).toHaveCount(4);

  const first = tiles.first();
  await expect(first).toHaveCSS("flex-direction", "column");

  const geom = await tiles.evaluateAll((els) =>
    els.map((el) => {
      const badge = el.querySelector("app-tool-badge")!.getBoundingClientRect();
      const label = el.querySelector(":scope > span")!.getBoundingClientRect();
      const box = el.getBoundingClientRect();
      return { w: Math.round(box.width), h: Math.round(box.height), badge: badge.bottom, label: label.top };
    }),
  );
  for (const t of geom) {
    // stacked, not side by side
    expect(t.badge).toBeLessThanOrEqual(t.label + 1);
  }
  // one grid of four equal cells — a "not found" tile must not grow taller
  expect(new Set(geom.map((t) => t.w)).size).toBe(1);
  expect(new Set(geom.map((t) => t.h)).size).toBe(1);
});

test("the four agent tiles stay on one row", async ({ page }) => {
  await openSpawn(page);
  const tops = await page
    .locator("app-spawn-modal .tool-tile .kj-button")
    .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().top)));
  expect(tops).toHaveLength(4);
  expect(new Set(tops).size).toBe(1);
});

test("Reasoning effort marks the chosen level as selected", async ({ page }) => {
  await openSpawn(page);
  // Claude exposes no effort levels — the field only exists for a tool that
  // does, so pick Codex first (this also covers the tile picker driving it)
  await page.locator("app-spawn-modal .tool-tile", { hasText: "Codex" }).click();

  const tabs = page.locator("app-spawn-modal .spawn-seg");
  const chosen = () => tabs.locator('.kj-tab[aria-selected="true"]');

  // exactly one level is selected on open, and it is the prefilled effort
  await expect(chosen()).toHaveCount(1);
  const initial = await page.evaluate(
    `window.ng.getComponent(document.querySelector("app-spawn-modal")).effort()`,
  );
  await expect(chosen()).toHaveText(new RegExp(`^${initial}$`, "i"));

  // picking another level moves the selection AND paints it. As four outline
  // kj-buttons nothing was painted at all: kouji declares --kj-button-bg/-fg/
  // -border-color ON the inner .kj-button per [data-variant], which beats the
  // inherited value a host-level [style.--kj-button-*] sets.
  const next = tabs.locator('.kj-tab[aria-selected="false"]').first();
  const label = (await next.textContent())!.trim();
  await next.click();

  await expect(chosen()).toHaveCount(1);
  await expect(chosen()).toHaveText(new RegExp(`^${label}$`, "i"));
  expect(
    await page.evaluate(
      `window.ng.getComponent(document.querySelector("app-spawn-modal")).effort()`,
    ),
  ).toBe(label);
  // poll: kouji adopts its constructable component sheets asynchronously, so
  // right after the click the pill can still be un-themed for a frame or two
  await expect
    .poll(() => chosen().evaluate((el) => getComputedStyle(el).backgroundColor), { timeout: 3000 })
    .not.toBe("rgba(0, 0, 0, 0)");
});

test("opening the dialog kicks off a branch refresh", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seed);
  // spy the refresh path BEFORE the modal mounts — its constructor calls it
  await page.evaluate(
    bar(`.projects.refresh = () => { window.__refreshed = true; return Promise.resolve(); }`),
  );
  await page.evaluate(bar(`.ui.openSpawn(null)`));
  await page.waitForSelector("app-spawn-modal", { state: "attached" });
  expect(await page.evaluate(`window.__refreshed === true`)).toBe(true);
});

test("the refresh button re-reads branches and keeps a still-valid selection", async ({ page }) => {
  await openSpawn(page);
  const refresh = page.locator('app-spawn-modal button[title="Refresh branches from disk"]');
  await expect(refresh).toBeVisible();
  await expect(refresh).toBeEnabled();

  // a branch appears on disk while the dialog is open: refresh() re-enriches
  // the project record (simulated by upserting the store, as project_list does)
  await page.evaluate(
    bar(`.projects.refresh = () => {
      const b = window.ng.getComponent(document.querySelector("app-top-bar"));
      const store = b.projects["projectsStore"]["store"];
      const p = store.all().find((x) => x.id === "7f3a91c4-2b5e-4d18-9a06-c1e8f4b7d302");
      store.upsert({ ...p, branches: [...p.branches, "feat/created-outside"] });
      return Promise.resolve();
    }`),
  );
  await refresh.click();

  // the selection survives (main still exists) and the new branch is offered
  const trigger = picker(page, 1);
  await expect(trigger).toHaveText(/main/);
  await trigger.click();
  await expect(panel(page).getByRole("option", { name: "feat/created-outside" })).toBeVisible();
});
