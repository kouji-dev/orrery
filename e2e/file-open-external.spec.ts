import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the file-tree "Open in Default App" / "Reveal in …" actions: the OS
 * hand-off a user reaches for when a generated `.html` should open in the
 * browser, or a file needs showing in Explorer.
 *
 * Backend-free: the real Tauri invoke is unavailable in a browser, so the spec
 * swaps the bridge's `invoke` for a recorder and asserts the command + payload
 * the menu items ship — which is the whole contract on the frontend side.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "C:/wt/${name}", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Put a ready file tree in the work store + record every bridge invoke. */
const seedTreeAndSpy = (id: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const work = bar.agentActions["work"];
  work["patch"](work["treesMap"], "${id}", {
    status: "ready",
    data: [{ name: "report.html", path: "docs/report.html", isDir: false, children: null, ignored: false }],
  });
  const tree = window.ng.getComponent(document.querySelector("app-file-tree"));
  const bridge = tree["bridge"];
  window.__calls = [];
  bridge.invoke = (command, args) => {
    window.__calls.push({ command, args });
    return Promise.resolve();
  };
})()`;

async function openTreeWithFile(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-os1", "e2e-os"));
  await page.evaluate(
    `window.ng.getComponent(document.querySelector("app-top-bar")).ui.openAgent("e2e-os1")`,
  );
  await page.locator("app-right-panel").getByRole("button", { name: "Files" }).click();
  await expect(page.locator("app-file-tree")).toBeVisible();
  await page.evaluate(seedTreeAndSpy("e2e-os1"));
  await expect(page.locator("app-file-tree")).toContainText("report.html");
}

test("Open in Default App sends the file to the OS handler, worktree-relative", async ({ page }) => {
  await openTreeWithFile(page);
  await page.locator("app-file-tree").getByText("report.html").click({ button: "right" });

  const menu = page.locator(".menu-panel");
  await expect(menu.getByRole("button", { name: "Open in Default App" })).toBeVisible();
  await menu.getByRole("button", { name: "Open in Default App" }).click();

  // the menu closes on action, and the invoke carries agent id + relative path
  await expect(menu).toHaveCount(0);
  expect(await page.evaluate("window.__calls")).toEqual([
    { command: "file_open_external", args: { id: "e2e-os1", path: "docs/report.html" } },
  ]);
});

test("Reveal shows the file in the platform's own file manager", async ({ page }) => {
  await openTreeWithFile(page);
  await page.locator("app-file-tree").getByText("report.html").click({ button: "right" });

  // the label names the actual OS file manager (Explorer / Finder / generic)
  const reveal = page.locator(".menu-panel button", { hasText: /^Reveal in / });
  await expect(reveal).toBeVisible();
  await reveal.click();

  expect(await page.evaluate("window.__calls")).toEqual([
    { command: "file_reveal", args: { id: "e2e-os1", path: "docs/report.html" } },
  ]);
});

test("the OS actions are node-scoped — the empty-space menu does not offer them", async ({
  page,
}) => {
  await openTreeWithFile(page);
  // right-click the panel header, not a row → root-scoped menu
  await page.locator("app-file-tree").locator("div").first().click({ button: "right" });

  const menu = page.locator(".menu-panel");
  await expect(menu.getByRole("button", { name: "New File…" })).toBeVisible();
  await expect(menu.getByRole("button", { name: "Open in Default App" })).toHaveCount(0);
  await expect(menu.locator("button", { hasText: /^Reveal in / })).toHaveCount(0);
});
