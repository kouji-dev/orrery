import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the v2 peek overlay — the unblock queue. It opens OVER the workspace
 * (dimmed, never replacing the active tab), walks pending notifications with
 * N / ⇧N, resolves permissions with Ctrl+Enter, and shows "Inbox zero" when
 * nothing is pending. The `n` key is inert while typing.
 *
 * Backend-free: notifications are seeded straight into NotificationStore.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "blocked", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedPermission = (agentId: string, agentName: string, cmd: string) => `(() => {
  const nc = window.ng.getComponent(document.querySelector("app-notification-center"));
  nc.notifications["store"].push({
    agentId: "${agentId}", agentName: "${agentName}", kind: "permission",
    title: "Permission needed", detail: "${cmd}", tool: "Bash", command: "${cmd}",
  });
})()`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-pk1", "alpha"));
  await page.evaluate(seedAgent("e2e-pk2", "beta"));
}

test("n opens the queue OVER the workspace; Esc closes it — tab unchanged", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("n");
  await expect(page.locator("app-peek-overlay [role=\"dialog\"]")).toBeVisible();
  // the workspace stays put behind the scrim — the overview is still mounted
  await expect(page.locator("app-overview")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("app-peek-overlay")).toHaveCount(0);
  await expect(page.locator("app-overview")).toBeVisible();
});

test("empty queue shows Inbox zero", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("n");
  await expect(page.locator("app-peek-overlay")).toContainText("Inbox zero");
  await expect(page.locator("app-peek-overlay")).toContainText("0 pending");
});

test("N walks the pending queue; ⇧N walks back", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedPermission("e2e-pk1", "alpha", "npm install"));
  await page.evaluate(seedPermission("e2e-pk2", "beta", "rm -rf dist"));
  await page.keyboard.press("n");
  const peek = page.locator("app-peek-overlay");
  await expect(peek).toContainText("1 of 2");
  await expect(peek).toContainText("alpha");
  await page.keyboard.press("n");
  await expect(peek).toContainText("2 of 2");
  await expect(peek).toContainText("beta");
  await page.keyboard.press("Shift+N");
  await expect(peek).toContainText("1 of 2");
});

test("Ctrl+Enter allows the current permission and advances the queue", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedPermission("e2e-pk1", "alpha", "npm test"));
  await page.evaluate(seedPermission("e2e-pk2", "beta", "npm build"));
  await page.keyboard.press("n");
  const peek = page.locator("app-peek-overlay");
  await expect(peek).toContainText("1 of 2");
  await page.keyboard.press("Control+Enter");
  // alpha resolved → the queue shrinks to beta's item
  await expect(peek).toContainText("1 of 1");
  await expect(peek).toContainText("beta");
});

test("the pending request renders as the shared notification card", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedPermission("e2e-pk1", "alpha", "cargo check"));
  await page.keyboard.press("n");
  const card = page.locator("app-peek-overlay app-notification-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("cargo check");
});

test("n is inert while typing in an input", async ({ page }) => {
  await boot(page);
  await page.locator("app-sidebar input[placeholder='filter agents…']").click();
  await page.keyboard.type("n");
  await expect(page.locator("app-peek-overlay")).toHaveCount(0);
  await expect(page.locator("app-sidebar input[placeholder='filter agents…']")).toHaveValue("n");
});

test("the bell dropdown offers Work the queue; it opens the peek", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedPermission("e2e-pk1", "alpha", "ls"));
  await page.locator("app-notification-center button[title='Notifications']").click();
  await page.getByRole("button", { name: /Work the queue/ }).click();
  await expect(page.locator("app-peek-overlay [role=\"dialog\"]")).toBeVisible();
});

test("status-bar 'need attention' opens the peek, not a tab switch", async ({ page }) => {
  await boot(page);
  await expect(page.locator("app-status-bar")).toContainText("need attention");
  await page.locator("app-status-bar button", { hasText: "need attention" }).click();
  await expect(page.locator("app-peek-overlay [role=\"dialog\"]")).toBeVisible();
  await expect(page.locator("app-overview")).toBeVisible(); // tab untouched
});
