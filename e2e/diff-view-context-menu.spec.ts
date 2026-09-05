import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the agent pane's diff file list context menu (the working-tree
 * "Changed · N" list, Tree and Flat views): right-clicking a file row offers
 * the two OS hand-offs — Open in Default App, Reveal in … — the same commands
 * the commit-diff list and the sidebar file tree ship. Folder rows and the
 * header are not files, so they open nothing; a deleted file has nothing on
 * disk, so its items are disabled.
 *
 * Backend-free: the changes are patched straight into the work store and the
 * shared bridge's `invoke` is swapped for a recorder, so the assertion is the
 * command + payload — the whole frontend contract.
 */

const AGENT = "e2e-dvcm";

const seedAgent = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${AGENT}", projectId: "p-e2e", tool: "claude", model: "m", name: "e2e-diff-menu",
    task: "", status: "idle", branch: "agent/e2e-diff-menu", worktree: "C:/wt/e2e-diff-menu", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const seedChanges = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: "x\\n" });
  const work = bar.agentActions["work"];
  work["patch"](work["changesMap"], "${AGENT}", {
    status: "ready",
    data: [
      { path: "docs/report.html", add: 3, del: 1, state: "M" },
      { path: "src/gone.ts", add: 0, del: 12, state: "D" },
    ],
  });
})()`;

const stubBridge = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const bridge = bar.agentActions["agentsStore"]["bridge"];
  window.__calls = [];
  bridge.invoke = (command, args) => {
    window.__calls.push({ command, args });
    return Promise.resolve();
  };
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openDiff(page: Page, mode: "tree" | "flat"): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent);
  await page.evaluate(seedChanges);
  await page.evaluate(stubBridge);
  await page.evaluate(ui(`.diffTreeMode.set(${mode === "tree"})`));
  await page.evaluate(ui(`.openAgent("${AGENT}", "diff")`));
  await expect(page.locator("app-diff-view")).toContainText("report.html");
}

const menu = (page: Page) => page.locator(".menu-panel");
const row = (page: Page, name: string) => page.locator("app-diff-view .diff-file", { hasText: name });

for (const mode of ["tree", "flat"] as const) {
  test(`${mode} view: right-clicking a file row offers Open in Default App and Reveal`, async ({ page }) => {
    await openDiff(page, mode);
    await row(page, "report.html").click({ button: "right" });

    const m = menu(page);
    await expect(m.getByRole("button", { name: "Open in Default App" })).toBeVisible();
    await expect(m.locator("button", { hasText: /^Reveal in / })).toBeVisible();
    // exactly the two OS hand-offs — no rename/delete here
    await expect(m.locator("button")).toHaveCount(2);
    // every item leads with its icon
    await expect(m.locator("button app-icon")).toHaveCount(2);
  });
}

test("Open in Default App sends the file to the OS handler, worktree-relative", async ({ page }) => {
  await openDiff(page, "tree");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).getByRole("button", { name: "Open in Default App" }).click();

  await expect(menu(page)).toHaveCount(0);
  const calls = await page.evaluate("window.__calls");
  expect(calls).toContainEqual({
    command: "file_open_external",
    args: { id: AGENT, path: "docs/report.html" },
  });
});

test("Reveal shows the file in the platform's own file manager", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).locator("button", { hasText: /^Reveal in / }).click();

  await expect(menu(page)).toHaveCount(0);
  const calls = await page.evaluate("window.__calls");
  expect(calls).toContainEqual({
    command: "file_reveal",
    args: { id: AGENT, path: "docs/report.html" },
  });
});

test("a deleted file has nothing on disk: both items are disabled", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "gone.ts").click({ button: "right" });
  const m = menu(page);
  await expect(m.getByRole("button", { name: "Open in Default App" })).toBeDisabled();
  await expect(m.locator("button", { hasText: /^Reveal in / })).toBeDisabled();
});

test("the menu is file-scoped — a folder row and the header open nothing", async ({ page }) => {
  await openDiff(page, "tree");
  await page.locator("app-diff-view .diff-dir", { hasText: "docs" }).click({ button: "right" });
  await expect(menu(page)).toHaveCount(0);
  await page.locator("app-diff-view").getByText("Changed ·").click({ button: "right" });
  await expect(menu(page)).toHaveCount(0);
});
