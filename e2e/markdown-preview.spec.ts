import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the markdown preview in the file tab: marked rendering, the
 * Raw/Preview toggle, and mermaid fences rendered to SVG (lazy chunk, exactly
 * like production — same pattern as the Monaco chunk in review-flow).
 *
 * The one invoke the file view needs (AgentDiff) is stubbed on AgentsStore;
 * every other invoke rejects and the view tolerates it (hunks/blame empty).
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const MD = "# Guide Title\n\nplain text body\n\n```mermaid\ngraph TD; A-->B;\n```\n";

/** Stub the working-tree read (AgentDiff `.new` side) with markdown content. */
const seedMd = () => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: ${JSON.stringify(MD)} });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openMd(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-md1", "e2e-markdown"));
  await page.evaluate(seedMd());
  await page.evaluate(ui(`.openFileInWorkspace("e2e-md1", "docs/guide.md")`));
  await expect(page.locator("app-file-view")).toBeVisible();
}

test("markdown opens rendered; mermaid fence becomes an SVG diagram", async ({ page }) => {
  await openMd(page);
  // marked output is live (Preview is the default)
  await expect(page.locator(".md-body h1", { hasText: "Guide Title" })).toBeVisible();

  // mermaid arrives as a lazy chunk — allow the first-load fetch + render
  const diagram = page.locator(".md-body .mmd svg");
  await expect(diagram).toBeVisible({ timeout: 30_000 });
  // the fence itself was replaced by the rendered block
  await expect(page.locator(".md-body code.language-mermaid")).toHaveCount(0);
});

test("Raw/Preview toggle swaps between the editor and the rendered document", async ({ page }) => {
  await openMd(page);
  await expect(page.locator(".md-body")).toBeVisible();

  await page.getByRole("tab", { name: "Raw", exact: true }).click();
  await expect(page.locator(".md-body")).toHaveCount(0);
  await expect(page.locator("app-monaco-file-editor")).toBeVisible();

  await page.getByRole("tab", { name: "Preview", exact: true }).click();
  await expect(page.locator(".md-body h1", { hasText: "Guide Title" })).toBeVisible();
});
