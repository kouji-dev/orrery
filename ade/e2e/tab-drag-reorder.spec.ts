import { expect, Page, test } from "@playwright/test";

/**
 * E2E for dragging agent tabs along the top bar.
 *
 * The drag runs on pointer events (not HTML5 dnd, which the Windows webview
 * swallows while Tauri's own file-drop handling is on), so Playwright's mouse
 * drives it exactly the way a user does. Orchestrator and Backlog are fixed:
 * they cannot be picked up, and nothing can be dropped in front of them.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
  bar.ui.openAgent("${id}");
  return bar.ui.activeTab();
})()`;

const tabOrder = `window.ng.getComponent(document.querySelector("app-top-bar")).ui.tabs().map((t) => t.id)`;
const activeTab = `window.ng.getComponent(document.querySelector("app-top-bar")).ui.activeTab()`;

const tab = (page: Page, id: string) => page.locator(`app-top-bar [data-tab-id="${id}"]`);

/** Press on `src`, glide to a point inside `dst` (at `frac` of its width), release. */
async function dragTab(page: Page, src: string, dst: string, frac: number): Promise<void> {
  const s = (await tab(page, src).boundingBox())!;
  const d = (await tab(page, dst).boundingBox())!;
  const y = s.y + s.height / 2;
  await page.mouse.move(s.x + s.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(s.x + s.width / 2 + 12, y, { steps: 3 });
  await page.mouse.move(d.x + d.width * frac, d.y + d.height / 2, { steps: 8 });
  await page.mouse.up();
}

async function boot(page: Page): Promise<{ a: string; b: string; c: string }> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  const a = (await page.evaluate(seedAgent("e2e-dnd-a", "alpha"))) as string;
  const b = (await page.evaluate(seedAgent("e2e-dnd-b", "bravo"))) as string;
  const c = (await page.evaluate(seedAgent("e2e-dnd-c", "charlie"))) as string;
  expect(await page.evaluate(tabOrder)).toEqual(["orchestrator", "backlog", a, b, c]);
  return { a, b, c };
}

test("an agent tab dropped on the leading edge of another moves in front of it", async ({ page }) => {
  const { a, b, c } = await boot(page);
  await dragTab(page, c, a, 0.1);
  expect(await page.evaluate(tabOrder)).toEqual(["orchestrator", "backlog", c, a, b]);
  // the release did not double as a click on the dragged tab
  expect(await page.evaluate(activeTab)).toBe(c);
});

test("an agent tab dropped on the trailing edge of another moves behind it", async ({ page }) => {
  const { a, b, c } = await boot(page);
  await dragTab(page, a, c, 0.9);
  expect(await page.evaluate(tabOrder)).toEqual(["orchestrator", "backlog", b, c, a]);
});

test("dropping on the middle of another agent tab tiles the two together", async ({ page }) => {
  const { a, b, c } = await boot(page);
  await dragTab(page, c, b, 0.5);
  expect(await page.evaluate(tabOrder)).toEqual(["orchestrator", "backlog", a, b]);
  await expect(tab(page, b)).toContainText("+1");
});

test("Orchestrator and Backlog stay put: not draggable, not a landing spot", async ({ page }) => {
  const { a, b, c } = await boot(page);
  await dragTab(page, "backlog", c, 0.9);
  await dragTab(page, "orchestrator", c, 0.9);
  expect(await page.evaluate(tabOrder)).toEqual(["orchestrator", "backlog", a, b, c]);
  await dragTab(page, c, "backlog", 0.1);
  await dragTab(page, c, "orchestrator", 0.1);
  expect(await page.evaluate(tabOrder)).toEqual(["orchestrator", "backlog", a, b, c]);
});

test("a plain click on a tab still selects it", async ({ page }) => {
  const { a, c } = await boot(page);
  expect(await page.evaluate(activeTab)).toBe(c);
  await tab(page, a).click();
  expect(await page.evaluate(activeTab)).toBe(a);
});
