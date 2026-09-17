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

/* ── mermaid robustness, tables, annotate (markdown_preview branch) ──────── */

/** Open the file view with arbitrary markdown (or text) content. */
async function openWith(page: Page, path: string, content: string): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-md2", "e2e-md-robust"));
  await page.evaluate(`(() => {
    const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
    bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: ${JSON.stringify(content)} });
  })()`);
  await page.evaluate(ui(`.openFileInWorkspace("e2e-md2", ${JSON.stringify(path)})`));
  await expect(page.locator("app-file-view")).toBeVisible();
}

test("a mermaid fence with a mis-indented closer no longer swallows the document", async ({ page }) => {
  // the closer is indented four spaces — not a CommonMark closer, and the exact
  // LLM slip that used to turn everything after the diagram into raw text
  const md = "# Top\n\n```mermaid\ngraph TD; A-->B;\n    ```\n\n## After\n\nStill a paragraph.\n";
  await openWith(page, "docs/flow.md", md);
  await expect(page.locator(".md-body h2", { hasText: "After" })).toBeVisible();
  await expect(page.locator(".md-body p", { hasText: "Still a paragraph." })).toBeVisible();
  await expect(page.locator(".md-body .mmd svg")).toBeVisible({ timeout: 30_000 });
});

test("an invalid diagram shows an error box and leaves the rest of the document intact", async ({ page }) => {
  const md = "# Top\n\n```mermaid\ngraph TD; A-->\n```\n\n## After\n\nStill here.\n";
  await openWith(page, "docs/bad.md", md);
  const err = page.locator(".md-body .md-box.error");
  await expect(err).toBeVisible({ timeout: 30_000 });
  await expect(err).toContainText("Diagram failed to render");
  await expect(err).toContainText("the rest of this document is unaffected");
  // the block failed, not the page
  await expect(page.locator(".md-body h2", { hasText: "After" })).toBeVisible();
  await expect(page.locator(".md-body .mmd svg")).toHaveCount(0);
});

test("GFM tables render as real tables that scroll in their own box", async ({ page }) => {
  const wide = "| " + Array.from({ length: 9 }, (_, i) => "a very long column header number " + i).join(" | ") + " |";
  const sep = "|" + " --- |".repeat(9);
  const row = "| " + Array.from({ length: 9 }, (_, i) => "cell " + i).join(" | ") + " |";
  const md = "# T\n\n" + [wide, sep, row, row].join("\n") + "\n\nafter\n";
  await openWith(page, "docs/table.md", md);

  const table = page.locator(".md-body .md-box.table table");
  await expect(table).toBeVisible();
  await expect(table.locator("th")).toHaveCount(9);
  await expect(table.locator("tbody tr")).toHaveCount(2);

  // cells are padded and bordered — the reported bug was flush, borderless text
  const pad = await table.locator("td").first().evaluate((el) => parseFloat(getComputedStyle(el).paddingLeft));
  expect(pad).toBeGreaterThan(0);

  // the wide table scrolls INSIDE its box; the pane itself never grows sideways
  const box = page.locator(".md-body .md-box.table .box-scroll");
  const [boxScroll, boxClient, bodyScroll, bodyClient] = await Promise.all([
    box.evaluate((el) => el.scrollWidth),
    box.evaluate((el) => el.clientWidth),
    page.locator(".md-body").evaluate((el) => el.scrollWidth),
    page.locator(".md-body").evaluate((el) => el.clientWidth),
  ]);
  expect(boxScroll).toBeGreaterThan(boxClient);
  expect(bodyScroll).toBeLessThanOrEqual(bodyClient + 1);
});

test("a pipe table missing its separator row still renders as a table", async ({ page }) => {
  const md = "# T\n\n| name | count |\n| alpha | 1 |\n| beta | 2 |\n";
  await openWith(page, "docs/loose.md", md);
  const table = page.locator(".md-body .md-box.table table");
  await expect(table).toBeVisible();
  await expect(table.locator("th")).toHaveCount(2);
  await expect(table.locator("tbody tr")).toHaveCount(2);
});

test("Annotate reports a failed blame instead of a blank pane", async ({ page }) => {
  // backend-free: the blame invoke rejects — the user must be told, not shown nothing
  await openWith(page, "src/app.ts", "const a = 1;\nconst b = 2;\n");
  await page.getByRole("button", { name: "Annotate" }).click();
  await expect(page.locator("app-file-view")).toContainText("blame failed:");
  await expect(page.locator("app-annotate-blame")).toHaveCount(0);
});

test("Annotate wins over Preview on a markdown file", async ({ page }) => {
  await openWith(page, "docs/guide2.md", "# Guide\n\nbody\n");
  await expect(page.locator(".md-body")).toBeVisible();
  await page.getByRole("button", { name: "Annotate" }).click();
  // the preview steps aside for the annotated source (here: its failure notice)
  await expect(page.locator(".md-body")).toHaveCount(0);
  await expect(page.locator("app-file-view")).toContainText("blame failed:");
  await page.getByRole("button", { name: "Annotate" }).click();
  await expect(page.locator(".md-body h1", { hasText: "Guide" })).toBeVisible();
});
