import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the instant create/remove feedback in the sidebar.
 *
 * The backend round trips behind Spawn/Create (git worktree checkout) and
 * Delete (folder removal) can take many seconds. The row must react at once:
 *   - spawn: a placeholder row with a spinner, under the id the frontend sent,
 *     replaced in place once the created agent lands under that id;
 *   - remove: the live row dims with a spinner until the delete settles, and
 *     comes back untouched when the backend refuses.
 *
 * Backend-free: the store's spawn/remove are swapped for deferred promises.
 */

const seedProject = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "${id}", name: "${name}", path: "C:/e2e/${name}", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main", defaultBranch: "main", branches: ["main"],
  });
})()`;

const seedAgent = (projectId: string, id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "${projectId}", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "C:/wt/${name}", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Replace the store's `spawn` with a promise the test settles by hand. */
const stubSpawn = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.agentActions["agentsStore"];
  window.__spawn = {};
  store.spawn = (req) => new Promise((resolve, reject) => {
    window.__spawn.req = req;
    window.__spawn.resolve = () => {
      const ag = { ...req, status: "idle", branch: "agent/" + req.name, worktree: "C:/wt/" + req.name,
        commits: 0, elapsed: 0, progress: 0, pending: [] };
      store["store"].upsert(ag);   // what agent://created does
      resolve(ag);
    };
    window.__spawn.reject = (msg) => reject(new Error(msg));
  });
})()`;

const stubRemove = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.agentActions["agentsStore"];
  window.__remove = {};
  store.remove = (id) => new Promise((resolve, reject) => {
    window.__remove.resolve = () => { store["store"].remove(id); resolve(); };   // agent://deleted
    window.__remove.reject = (msg) => reject(new Error(msg));
  });
})()`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject("p-pend", "pending-proj"));
}

const rows = (page: Page) => page.locator("app-sidebar app-agent-row");
const pendingRow = (page: Page) => page.locator("app-sidebar app-agent-row .agent-row.pending");

test("spawn shows a creating placeholder at once, replaced in place when the agent lands", async ({ page }) => {
  await boot(page);
  await page.evaluate(stubSpawn);
  await expect(rows(page)).toHaveCount(0);

  await page.evaluate(`void window.ng.getComponent(document.querySelector("app-top-bar")).agentActions.spawn({
    projectId: "p-pend", branch: "main", toolId: "claude", model: "m", effort: null,
    name: "fresh-agent", prompt: "hello", start: false,
  })`);

  // placeholder: spinner, dimmed, not interactive, labelled as creating
  await expect(pendingRow(page)).toHaveCount(1, { timeout: 500 });
  await expect(pendingRow(page).locator("kj-spinner")).toBeVisible();
  await expect(pendingRow(page)).toContainText("fresh-agent");
  await expect(pendingRow(page)).toContainText("creating worktree…");
  await expect(pendingRow(page)).toHaveCSS("pointer-events", "none");
  await expect(rows(page).first().locator(".agent-row")).not.toHaveAttribute("draggable", "true");

  // the request carried the placeholder's id
  const sentId = await page.evaluate("window.__spawn.req.id");
  expect(typeof sentId).toBe("string");
  expect((sentId as string).length).toBeGreaterThan(20);

  await page.evaluate("window.__spawn.resolve()");
  await expect(pendingRow(page)).toHaveCount(0);
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("fresh-agent");
  await expect(rows(page).first().locator("kj-spinner")).toHaveCount(0);
});

test("a failed spawn drops the placeholder again", async ({ page }) => {
  await boot(page);
  await page.evaluate(stubSpawn);
  await page.evaluate(`void window.ng.getComponent(document.querySelector("app-top-bar")).agentActions.spawn({
    projectId: "p-pend", branch: "main", toolId: "claude", model: "m", effort: null,
    name: "doomed", prompt: "", start: false,
  })`);
  await expect(pendingRow(page)).toHaveCount(1);
  await page.evaluate(`window.__spawn.reject("worktree add: boom")`);
  await expect(rows(page)).toHaveCount(0);
});

test("remove dims the row with a spinner immediately, and restores it when the backend refuses", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedAgent("p-pend", "e2e-pend-1", "doomed"));
  await expect(rows(page)).toHaveCount(1);
  await page.evaluate(stubRemove);

  await page.evaluate(`void window.ng.getComponent(document.querySelector("app-top-bar")).agentActions.confirmRemoveAgent("e2e-pend-1", true)`);
  await expect(pendingRow(page)).toHaveCount(1, { timeout: 500 });
  await expect(pendingRow(page)).toContainText("removing…");
  await expect(pendingRow(page).locator("kj-spinner")).toBeVisible();

  await page.evaluate(`window.__remove.reject("locked")`);
  await expect(pendingRow(page)).toHaveCount(0);
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("agent/doomed".replace("agent/", ""));
});

test("remove: the row leaves once the backend confirms", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedAgent("p-pend", "e2e-pend-2", "going"));
  await page.evaluate(stubRemove);
  await page.evaluate(`void window.ng.getComponent(document.querySelector("app-top-bar")).agentActions.confirmRemoveAgent("e2e-pend-2")`);
  await expect(pendingRow(page)).toHaveCount(1);
  await page.evaluate(`window.__remove.resolve()`);
  await expect(rows(page)).toHaveCount(0);
});
