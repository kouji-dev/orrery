import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the diff header's Preview action: when the selected changed file is
 * previewable (markdown), a Preview button appears beside Annotate/Commit/Merge
 * and opens that file as a workspace file tab in rendered preview mode — the
 * exact same path as clicking the file in the right-panel file tree.
 *
 * The one invoke both views need (AgentDiff) is stubbed on AgentsStore; the
 * changed-file list is seeded straight into AgentWorkStore.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const MD = "# Guide Title\\n\\nplain text body\\n";

/** Stub the diff invoke (md content for .md paths) + seed the changed files. */
const seedChanges = (id: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = (agentId, path) => Promise.resolve(
    /\\.md$/i.test(path)
      ? { old: "", new: "${MD}" }
      : { old: "const a = 1;\\n", new: "const a = 2;\\n" },
  );
  const work = bar.agentActions["work"];
  work["patch"](work["changesMap"], "${id}", {
    status: "ready",
    data: [
      { path: "docs/guide.md", add: 3, del: 0, state: "M" },
      { path: "docs/old.md", add: 0, del: 9, state: "D" },
      { path: "src/example.ts", add: 1, del: 1, state: "M" },
    ],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openDiff(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-pv1", "e2e-preview"));
  await page.evaluate(seedChanges("e2e-pv1"));
  await page.evaluate(ui(`.openAgent("e2e-pv1", "diff")`));
  await expect(page.locator(".diff-head")).toBeVisible();
}

const previewBtn = (page: Page) =>
  page.locator(".diff-head").getByRole("button", { name: "Preview" });

test("selecting a markdown file surfaces Preview in the diff header", async ({ page }) => {
  await openDiff(page);
  // guide.md is the first changed file → selected by default
  await expect(page.locator(".diff-head-path")).toContainText("docs/guide.md");
  await expect(previewBtn(page)).toBeVisible();
});

test("Preview opens the file tab rendered, like the right-panel file tree", async ({ page }) => {
  await openDiff(page);
  await previewBtn(page).click();

  // the leaf switched to the file tab and markdown opens in Preview mode
  await expect(page.locator("app-file-view")).toBeVisible();
  await expect(page.locator(".md-body h1", { hasText: "Guide Title" })).toBeVisible();
});

test("no Preview for non-previewable or deleted files", async ({ page }) => {
  await openDiff(page);

  // a .ts file: header actions stay, Preview does not appear
  await page.locator(".diff-file", { hasText: "example.ts" }).click();
  await expect(page.locator(".diff-head-path")).toContainText("src/example.ts");
  await expect(page.locator(".diff-head").getByRole("button", { name: "Annotate" })).toBeVisible();
  await expect(previewBtn(page)).toHaveCount(0);

  // a deleted .md file: nothing in the worktree to render
  await page.locator(".diff-file", { hasText: "old.md" }).click();
  await expect(page.locator(".diff-head-path")).toContainText("docs/old.md");
  await expect(previewBtn(page)).toHaveCount(0);
});
