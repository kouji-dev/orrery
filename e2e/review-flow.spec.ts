import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the inline-review flow in the agent Diff tab: the hover-plus glyph
 * margin, the inline composer, saved-comment cards, and the send-review button.
 *
 * The one invoke the flow needs (AgentDiff) is stubbed on AgentsStore, so REAL
 * Monaco (the bundled lazy chunk, exactly like production) renders the inline
 * diff and the review-comment attachment runs unmodified.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Stub the diff invoke + seed one changed file into AgentWorkStore. */
const seedDiff = (id: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = () => Promise.resolve({
    old: "line one\\nline two\\nline three\\n",
    new: "line one\\nline two CHANGED\\nline three\\nline four added\\n",
  });
  const work = bar.agentActions["work"];
  work["patch"](work["changesMap"], "${id}", {
    status: "ready",
    data: [{ path: "src/example.ts", add: 2, del: 1, state: "M" }],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openDiff(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-rv1", "e2e-review"));
  await page.evaluate(seedDiff("e2e-rv1"));
  await page.evaluate(ui(`.openAgent("e2e-rv1", "diff")`));
  // Monaco arrives as a lazy chunk — allow the first-load fetch + mount.
  // (.monaco-editor alone would match the diff editor's hidden gutter node.)
  await expect(page.locator(".monaco-diff-editor").first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".view-line", { hasText: "line two CHANGED" })).toBeVisible({ timeout: 15_000 });
}

test("hovering a diff line shows the + glyph in the margin", async ({ page }) => {
  await openDiff(page);
  // hover a content line → the glyph margin grows a + for that row (exact
  // count — the Monaco attachment has no hidden spacer marker)
  await page.locator(".view-line", { hasText: "line two CHANGED" }).hover();
  await expect(page.locator(".rc-glyph-plus")).toHaveCount(1);
});

test("clicking + opens the composer; saving renders the card and arms send-review", async ({ page }) => {
  await openDiff(page);
  await page.locator(".view-line", { hasText: "line two CHANGED" }).hover();
  const plus = page.locator(".rc-glyph-plus").first();
  await expect(plus).toBeVisible();
  // real press-release on the glyph (the attachment listens on editor
  // mousedown in the glyph margin + window mouseup)
  await plus.hover();
  // the glyph is hover-driven: confirm it survived the pointer move onto it
  // before pressing, or a slow repaint swallows the mousedown
  await expect(plus).toBeVisible();
  await page.mouse.down();
  await page.mouse.up();

  const composer = page.locator(".rc-composer");
  await expect(composer).toBeVisible();
  await composer.locator(".rc-composer-ta").fill("please rename this variable");
  // the composer is a Monaco view zone: native buttons wearing the kouji skin.
  // Monaco marks the zone container aria-hidden, so getByRole cannot see them —
  // address the button by class.
  await composer.locator("button.rc-composer-btn", { hasText: "Save" }).click();

  // saved card renders inline; composer closes
  await expect(page.locator(".rc-card")).toBeVisible();
  await expect(page.locator(".rc-card")).toContainText("please rename this variable");
  await expect(composer).toHaveCount(0);

  // the diff header's send-review button now carries the pending comment
  const send = page.locator("app-send-review-button");
  await expect(send).toContainText("1");
});

test("diff header carries agent-level actions: commit (AI), rebase (AI), native merge", async ({ page }) => {
  await openDiff(page);
  const head = page.locator(".diff-head");
  const buttons = head.locator("app-git-action-button");
  await expect(buttons).toHaveCount(3);

  // commit + rebase are aiOnly — no inline estimate while the cost switch is off
  const commit = buttons.filter({ hasText: "Commit" }).first();
  await expect(commit.locator(".est-inline")).toHaveCount(0);
  const rebase = buttons.filter({ hasText: "Rebase onto" }).first();
  await expect(rebase.locator(".est-inline")).toHaveCount(0);

  // merge: primary press is native; the AI variant row renders without a price
  const merge = buttons.filter({ hasText: "Merge" }).first();
  await merge.locator(".caret").click();
  // the dropdown content is portalled out of app-git-action-button
  const row = page.locator("kj-dropdown-menu-content.menu:visible kj-button.row", { hasText: "Merge with AI" });
  await expect(row).toBeVisible();
  await expect(row.locator(".row-est")).toHaveCount(0);
  await page.keyboard.press("Escape");

  // send review still lives beside them
  await expect(head.locator("app-send-review-button")).toHaveCount(1);
});
