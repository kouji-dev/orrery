import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the markdown preview's SELECTION → NOTE feedback loop and the top-bar
 * feedback chip: select prose in the rendered .md, the circle note button
 * appears after the selection, its composer saves a review comment anchored to
 * the source lines, the saved passage is marked, and the global chip counts it.
 *
 * Backend-free: the file read (AgentDiff `.new`) is stubbed on AgentsStore, the
 * review store is in-memory. Nothing is ever sent — OK's PTY write rejects and
 * the spec only asserts the queue, never a fabricated agent reply.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const MD = [
  "# Retry design",
  "",
  "Deliveries are keyed on the event id, so a redelivered event never runs twice.",
  "",
  "## Limits",
  "",
  "Cap at five attempts before dead-lettering the payload.",
  "",
].join("\n");

const seedMd = () => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: ${JSON.stringify(MD)} });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openMd(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-fb1", "e2e-feedback"));
  await page.evaluate(seedMd());
  await page.evaluate(ui(`.openFileInWorkspace("e2e-fb1", "docs/retry.md")`));
  await expect(page.locator(".md-body h1", { hasText: "Retry design" })).toBeVisible();
  // the render pass stamps source lines onto the blocks (data-l0) in the same
  // tick the selection overlay goes live — a selection made before that lands
  // on a page that cannot answer it yet
  await page.waitForSelector(".md-body .rte-view [data-l0]");
  // the agent tab focused xterm; plain chords and selection must not go there
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

/** Select `text` inside the first rendered block that contains it, then fire
 *  the mouseup the overlay listens for — a real drag is what a user does, but
 *  a programmatic Range + mouseup exercises the same code path deterministically. */
async function selectText(page: Page, text: string): Promise<void> {
  // under a loaded machine the first mouseup can still race the listener;
  // re-selecting is exactly what a user does, so retry until the button shows
  await expect
    .poll(async () => {
      await selectOnce(page, text);
      return page.locator(".md-body .md-note-btn").count();
    }, { timeout: 15_000, intervals: [250, 500, 1000] })
    .toBe(1);
}

async function selectOnce(page: Page, text: string): Promise<void> {
  await page.evaluate((needle) => {
    const body = document.querySelector(".md-body .rte-view")!;
    const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
    let node: Node | null;
    while ((node = walker.nextNode())) {
      const at = node.nodeValue!.indexOf(needle);
      if (at < 0) continue;
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + needle.length);
      const sel = window.getSelection()!;
      sel.removeAllRanges();
      sel.addRange(range);
      document.querySelector(".md-body")!.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      return;
    }
    throw new Error("text not found: " + needle);
  }, text);
}

test("selecting prose reveals the note button after the selection", async ({ page }) => {
  await openMd(page);
  const btn = page.locator(".md-body .md-note-btn");
  await expect(btn).toHaveCount(0);
  await selectText(page, "never runs twice");
  await expect(btn).toBeVisible();
  await expect(btn).toHaveAttribute("title", "Comment on selection");
});

test("the composer saves a comment: passage marked, chip counts it, popover lists it", async ({ page }) => {
  await openMd(page);
  await selectText(page, "never runs twice");
  // mousedown, not click — the button must beat the selection collapse
  await page.locator(".md-body .md-note-btn").dispatchEvent("mousedown");

  const composer = page.locator(".md-body .rc-composer");
  await expect(composer).toBeVisible();
  // anchored to the SOURCE line of the selected paragraph
  await expect(composer).toContainText("line 3");
  const ta = composer.locator("textarea");
  await expect(ta).toBeFocused();
  await expect(composer.getByRole("button", { name: "Save" })).toBeDisabled();
  await ta.fill("say what happens when the id is missing");
  await ta.press("Control+Enter");

  // saved: the passage is wrapped and carries a marker
  await expect(composer).toHaveCount(0);
  await expect(page.locator(".md-body .md-hl")).toContainText("never runs twice");
  await expect(page.locator(".md-body .md-mark")).toHaveCount(1);

  // the global chip counts it, and its popover lists file + line + note
  // the chip is a kouji host (display:contents) — assert on it, click its button
  const chip = page.locator("app-review-chip .fb-chip");
  await expect(chip).toContainText("1");
  await chip.locator(".kj-button").click();
  const pop = page.locator(".fb-pop");
  await expect(pop).toBeVisible();
  await expect(pop).toContainText("retry.md");
  await expect(pop).toContainText(":3");
  await expect(pop).toContainText("say what happens when the id is missing");

  // Cancel dismisses only — the comment stays queued
  await pop.getByRole("button", { name: "Cancel" }).click();
  await expect(pop).toHaveCount(0);
  await expect(chip).toContainText("1");
});

test("escape cancels the composer and drops the selection wrap", async ({ page }) => {
  await openMd(page);
  await selectText(page, "five attempts");
  await page.locator(".md-body .md-note-btn").dispatchEvent("mousedown");
  const composer = page.locator(".md-body .rc-composer");
  await expect(composer).toBeVisible();
  await expect(page.locator(".md-body .md-sel")).toHaveCount(1);
  await composer.locator("textarea").press("Escape");
  await expect(composer).toHaveCount(0);
  await expect(page.locator(".md-body .md-sel")).toHaveCount(0);
  await expect(page.locator("app-review-chip .fb-chip")).toHaveCount(0);
});

test("hovering a saved mark shows the card; Remove clears it and the chip", async ({ page }) => {
  await openMd(page);
  await selectText(page, "five attempts");
  await page.locator(".md-body .md-note-btn").dispatchEvent("mousedown");
  await page.locator(".md-body .rc-composer textarea").fill("make the cap configurable");
  await page.locator(".md-body .rc-composer textarea").press("Control+Enter");
  await expect(page.locator(".md-body .md-mark")).toHaveCount(1);

  await page.locator(".md-body .md-mark").hover();
  const card = page.locator(".md-body .md-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("make the cap configurable");
  await card.getByRole("button", { name: "Remove" }).click();
  await expect(page.locator(".md-body .md-mark")).toHaveCount(0);
  await expect(page.locator("app-review-chip .fb-chip")).toHaveCount(0);
});
