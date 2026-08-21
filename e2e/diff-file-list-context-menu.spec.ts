import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the diff-file-list context menu (left file list in commit/range diff
 * views): the same 4 file actions as the right-panel file tree's — Rename,
 * Open in Default App, Reveal in …, Delete — but scoped to a specific file
 * row only. Unlike the file tree, this list has no root/empty-space menu:
 * there's nothing to create here, so right-clicking anywhere but a file row
 * must not open anything.
 *
 * Backend-free: swaps the shared BRIDGE singleton's `invoke` for a recorder —
 * it also serves the commit's file list, since the real Tauri invoke that
 * loads it is unavailable in a browser.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "C:/wt/${name}", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

/** Swap the shared bridge singleton: the commit-files load resolves with one
 *  file, every other invoke (the row actions) resolves and is recorded. */
const stubBridge = () => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const bridge = bar.agentActions["agentsStore"]["bridge"];
  window.__calls = [];
  bridge.invoke = (command, args) => {
    window.__calls.push({ command, args });
    if (command === "agent_commit_diff") {
      return Promise.resolve([{ path: "docs/report.html", state: "M", add: 3, del: 1 }]);
    }
    return Promise.resolve();
  };
})()`;

async function openCommitDiffWithFile(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-dl1", "e2e-difflist"));
  await page.evaluate(stubBridge());
  await page.evaluate(ui(`.openAgent("e2e-dl1", "diff")`));
  await page.evaluate(ui(`.setGitView("e2e-dl1", { kind: "commit", sha: "deadbeef" })`));
  await expect(page.locator("app-diff-file-list")).toContainText("report.html");
}

test("right-clicking a file row opens the same 4 actions as the file tree", async ({ page }) => {
  await openCommitDiffWithFile(page);
  await page.locator("app-diff-file-list").getByText("report.html").click({ button: "right" });

  const menu = page.locator(".menu-panel");
  await expect(menu.getByRole("button", { name: "Rename…" })).toBeVisible();
  await expect(menu.getByRole("button", { name: "Open in Default App" })).toBeVisible();
  await expect(menu.locator("button", { hasText: /^Reveal in / })).toBeVisible();
  await expect(menu.getByRole("button", { name: "Delete" })).toBeVisible();
});

test("the menu is file-scoped — right-clicking the header opens nothing", async ({ page }) => {
  await openCommitDiffWithFile(page);
  await page.locator("app-diff-file-list").getByText("Files in commit", { exact: false }).click({ button: "right" });
  await expect(page.locator(".menu-panel")).toHaveCount(0);
});

test("Open in Default App sends the file to the OS handler, worktree-relative", async ({ page }) => {
  await openCommitDiffWithFile(page);
  await page.locator("app-diff-file-list").getByText("report.html").click({ button: "right" });
  const menu = page.locator(".menu-panel");
  await menu.getByRole("button", { name: "Open in Default App" }).click();

  await expect(menu).toHaveCount(0);
  const calls = await page.evaluate("window.__calls");
  expect(calls).toContainEqual({
    command: "file_open_external",
    args: { id: "e2e-dl1", path: "docs/report.html" },
  });
});

test("Reveal shows the file in the platform's own file manager", async ({ page }) => {
  await openCommitDiffWithFile(page);
  await page.locator("app-diff-file-list").getByText("report.html").click({ button: "right" });
  const menu = page.locator(".menu-panel");
  await menu.locator("button", { hasText: /^Reveal in / }).click();

  const calls = await page.evaluate("window.__calls");
  expect(calls).toContainEqual({
    command: "file_reveal",
    args: { id: "e2e-dl1", path: "docs/report.html" },
  });
});

test("Rename commits a new path via file_rename", async ({ page }) => {
  await openCommitDiffWithFile(page);
  await page.locator("app-diff-file-list").getByText("report.html").click({ button: "right" });
  await page.locator(".menu-panel").getByRole("button", { name: "Rename…" }).click();

  const input = page.locator(".menu-input");
  await expect(input).toHaveValue("report.html");
  await input.fill("summary.html");
  await input.press("Enter");

  await expect(page.locator(".menu-panel")).toHaveCount(0);
  const calls = await page.evaluate("window.__calls");
  expect(calls).toContainEqual({
    command: "file_rename",
    args: { id: "e2e-dl1", from: "docs/report.html", to: "docs/summary.html" },
  });
});

test("Delete asks for confirmation, then sends file_delete", async ({ page }) => {
  await openCommitDiffWithFile(page);
  await page.locator("app-diff-file-list").getByText("report.html").click({ button: "right" });
  await page.locator(".menu-panel").getByRole("button", { name: "Delete" }).click();

  await expect(page.locator(".menu-panel")).toContainText("report.html");
  await page.locator(".menu-panel").getByRole("button", { name: "Delete" }).click();

  await expect(page.locator(".menu-panel")).toHaveCount(0);
  const calls = await page.evaluate("window.__calls");
  expect(calls).toContainEqual({
    command: "file_delete",
    args: { id: "e2e-dl1", path: "docs/report.html" },
  });
});
