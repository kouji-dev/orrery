import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the agent pane's changed-file list context menu (the working-tree
 * "Changed · N" list, Tree and Flat views). It carries the same file CRUD the
 * sidebar file tree ships — New File…, New Folder…, Rename…, Delete — plus the
 * two OS hand-offs (Open in Default App, Reveal in …).
 *
 * The menu opens from three places, and where it opened decides where a create
 * lands: a file row → the file's parent folder, a folder row → inside it, the
 * empty space below the rows → the worktree root. A folder row here IS a real
 * worktree folder (only the path segment is synthesised by the tree), so its
 * rename/delete act on disk. A deleted file has nothing left on disk, so its
 * two OS hand-offs disable.
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
  // the listing commands must answer with a LIST, exactly as the backend does:
  // a write re-scans, and an undefined payload would poison the store
  const lists = ["agent_changes", "agent_tree", "agent_commits"];
  bridge.invoke = (command, args) => {
    window.__calls.push({ command, args });
    return Promise.resolve(lists.includes(command) ? [] : undefined);
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
const dirRow = (page: Page, name: string) => page.locator("app-diff-view .diff-dir", { hasText: name });
const calls = (page: Page) => page.evaluate("window.__calls");

/** Fill the menu's name field and confirm with OK. */
async function typeName(page: Page, name: string): Promise<void> {
  await menu(page).locator("input.menu-input").fill(name);
  await menu(page).getByRole("button", { name: "OK" }).click();
}

for (const mode of ["tree", "flat"] as const) {
  test(`${mode} view: a file row offers the full CRUD menu plus the OS hand-offs`, async ({ page }) => {
    await openDiff(page, mode);
    await row(page, "report.html").click({ button: "right" });

    const m = menu(page);
    for (const label of ["New File…", "New Folder…", "Rename…", "Open in Default App", "Delete"]) {
      await expect(m.getByRole("button", { name: label })).toBeVisible();
    }
    await expect(m.locator("button", { hasText: /^Reveal in / })).toBeVisible();
    // every item leads with its icon (locked rule)
    await expect(m.locator("button app-icon")).toHaveCount(6);
  });
}

test("a create from a file row lands in that file's folder, and opens the new file", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).getByRole("button", { name: "New File…" }).click();
  await expect(menu(page)).toContainText("New file in docs");
  await typeName(page, "notes.md");

  await expect(menu(page)).toHaveCount(0);
  expect(await calls(page)).toContainEqual({
    command: "file_create",
    args: { id: AGENT, path: "docs/notes.md" },
  });
  // a fresh file is worth looking at — it opens pinned in the workspace
  await expect(page.locator("app-pane-manager .file-strip .file-tab", { hasText: "notes.md" })).toBeVisible();
});

test("a folder row scopes the create INSIDE it", async ({ page }) => {
  await openDiff(page, "tree");
  await dirRow(page, "src").click({ button: "right" });
  await menu(page).getByRole("button", { name: "New Folder…" }).click();
  await expect(menu(page)).toContainText("New folder in src");
  await typeName(page, "util");

  expect(await calls(page)).toContainEqual({
    command: "dir_create",
    args: { id: AGENT, path: "src/util" },
  });
});

test("empty space below the rows is the worktree root — creates only", async ({ page }) => {
  await openDiff(page, "flat");
  // the listing's own padding, well below the last row
  const list = page.locator("app-diff-view .scroll-y").first();
  const box = (await list.boundingBox())!;
  await list.click({ button: "right", position: { x: 40, y: box.height - 4 } });

  const m = menu(page);
  await expect(m.getByRole("button", { name: "New File…" })).toBeVisible();
  await expect(m.getByRole("button", { name: "New Folder…" })).toBeVisible();
  // nothing was pointed at, so nothing to rename, hand off, or delete
  await expect(m.locator("button")).toHaveCount(2);

  await m.getByRole("button", { name: "New File…" }).click();
  await expect(m).toContainText("New file in worktree root");
  await typeName(page, "TODO.md");
  expect(await calls(page)).toContainEqual({
    command: "file_create",
    args: { id: AGENT, path: "TODO.md" },
  });
});

test("Rename… keeps the file in its folder", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).getByRole("button", { name: "Rename…" }).click();
  await expect(menu(page)).toContainText("Rename report.html");
  await typeName(page, "summary.html");

  expect(await calls(page)).toContainEqual({
    command: "file_rename",
    args: { id: AGENT, from: "docs/report.html", to: "docs/summary.html" },
  });
});

test("a folder rename moves the folder itself, not its files", async ({ page }) => {
  await openDiff(page, "tree");
  await dirRow(page, "docs").click({ button: "right" });
  await menu(page).getByRole("button", { name: "Rename…" }).click();
  await typeName(page, "guides");

  expect(await calls(page)).toContainEqual({
    command: "file_rename",
    args: { id: AGENT, from: "docs", to: "guides" },
  });
});

test("Delete confirms first, then removes the file", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).getByRole("button", { name: "Delete" }).click();

  // the confirm step is kouji's <kj-confirm-popup>, portalled OUT of the menu.
  // Its content host is display:contents (no box), so assert on the message and
  // press the action slot's button.
  await expect(page.locator("kj-confirm-popup-message:visible")).toContainText("report.html");
  await page.locator("kj-confirm-popup-action button:visible").click();

  await expect(menu(page)).toHaveCount(0);
  expect(await calls(page)).toContainEqual({
    command: "file_delete",
    args: { id: AGENT, path: "docs/report.html" },
  });
});

test("deleting a folder says so — it takes the contents with it", async ({ page }) => {
  await openDiff(page, "tree");
  await dirRow(page, "src").click({ button: "right" });
  await menu(page).getByRole("button", { name: "Delete" }).click();
  await expect(page.locator("kj-confirm-popup-message:visible")).toContainText(
    "Delete src and its contents?",
  );
  await page.locator("kj-confirm-popup-action button:visible").click();

  expect(await calls(page)).toContainEqual({
    command: "file_delete",
    args: { id: AGENT, path: "src" },
  });
});

test("Open in Default App sends the file to the OS handler, worktree-relative", async ({ page }) => {
  await openDiff(page, "tree");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).getByRole("button", { name: "Open in Default App" }).click();

  await expect(menu(page)).toHaveCount(0);
  expect(await calls(page)).toContainEqual({
    command: "file_open_external",
    args: { id: AGENT, path: "docs/report.html" },
  });
});

test("Reveal shows the file in the platform's own file manager", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "report.html").click({ button: "right" });
  await menu(page).locator("button", { hasText: /^Reveal in / }).click();

  await expect(menu(page)).toHaveCount(0);
  expect(await calls(page)).toContainEqual({
    command: "file_reveal",
    args: { id: AGENT, path: "docs/report.html" },
  });
});

test("a deleted file has nothing on disk: both hand-offs are disabled", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "gone.ts").click({ button: "right" });
  const m = menu(page);
  await expect(m.getByRole("button", { name: "Open in Default App" })).toBeDisabled();
  await expect(m.locator("button", { hasText: /^Reveal in / })).toBeDisabled();
  // it is still a path: renaming and deleting it stay available
  await expect(m.getByRole("button", { name: "Rename…" })).toBeEnabled();
});


test("opening the menu leaves the diff body alone — the code view never wraps below the file list", async ({ page }) => {
  await openDiff(page, "flat");
  await row(page, "report.html").click();
  const head = page.locator("app-diff-view .diff-head");
  await expect(head).toBeVisible();
  const before = (await head.boundingBox())!;

  await row(page, "report.html").click({ button: "right" });
  await expect(menu(page)).toBeVisible();

  // regression: <app-menu-panel> renders as a direct child of .diff-grid. As an
  // in-flow box it counted as a 4th item of the `232px 6px 1fr` track list,
  // wrapping the diff body onto row 2 — the code view dropped off the bottom of
  // the pane and the open file looked empty. The host is out of flow now.
  const after = (await head.boundingBox())!;
  expect(after.x).toBeCloseTo(before.x, 0);
  expect(after.y).toBeCloseTo(before.y, 0);
  await expect(head).toBeInViewport();
});
