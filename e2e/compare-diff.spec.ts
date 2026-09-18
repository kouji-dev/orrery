import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the multi-commit compare view (graph selection → "Compare N
 * commits").
 *
 * It used to be a different surface from the diff panel it mirrors: a header
 * of bare sha chips that spilled once a selection grew, a file list with no
 * collapsible folders, and a fixed-width column. This pins the three:
 *
 *  1. the header names the commits (count, files, ±lines, endpoint subjects)
 *     and keeps the rest behind one disclosure that scrolls;
 *  2. the file list is THE shared list — folders collapse, exactly as in the
 *     working-changes panel;
 *  3. the column is resizable, on the SAME stored width as that panel.
 */

const AGENT = "e2e-cmp";

const seedAgent = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${AGENT}", projectId: "p-e2e", tool: "claude", model: "m", name: "e2e-compare",
    task: "", status: "idle", branch: "agent/e2e-compare", worktree: "C:/wt/cmp", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** N commits, newest first (the log's own order) — the header must re-order
 *  them oldest → newest, the way the backend compares them. */
const commitsOf = (n: number) =>
  Array.from({ length: n }, (_, i) => ({
    sha: `c${i}aaaaa000000000000000000000000000000000`.slice(0, 40),
    msg: `commit subject ${i}`,
    ts: 1_700_000_000 - i * 3600,
  }));

const stubBridge = (n: number) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const bridge = bar.agentActions["agentsStore"]["bridge"];
  const commits = ${JSON.stringify(commitsOf(n))}.map((c) => ({
    ...c, agent: "e2e-compare", projectId: "p-e2e", when: "today", files: 1,
  }));
  window.__calls = [];
  bridge.invoke = (command, args) => {
    window.__calls.push({ command, args });
    if (command === "agent_commits") return Promise.resolve(commits);
    if (command === "agent_commits_files") {
      return Promise.resolve([
        { path: "src/app/one.ts", state: "M", add: 10, del: 2,
          firstSha: commits[5].sha, lastSha: commits[0].sha, commits: 2 },
        { path: "src/app/deep/two.ts", state: "A", add: 5, del: 0,
          firstSha: commits[5].sha, lastSha: commits[0].sha, commits: 1 },
        { path: "README.md", state: "M", add: 1, del: 1,
          firstSha: commits[5].sha, lastSha: commits[0].sha, commits: 1 },
      ]);
    }
    if (command === "agent_commits_file_diff") {
      return Promise.resolve({ old: "a\\n", new: "b\\n", lang: "ts" });
    }
    return Promise.resolve([]);
  };
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openCompare(page: Page, n = 6): Promise<void> {
  const shas = commitsOf(n).map((c) => c.sha);
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent);
  await page.evaluate(stubBridge(n));
  await page.evaluate(ui(`.openAgent("${AGENT}", "diff")`));
  await page.evaluate(
    ui(`.setGitView("${AGENT}", { kind: "commits", shas: ${JSON.stringify(shas)} })`),
  );
  await expect(page.locator("app-range-diff-view")).toBeVisible();
  await expect(page.locator("app-diff-file-list")).toContainText("one.ts");
}

const head = (page: Page) => page.locator("app-range-diff-view .cmp-head");

test("the header counts the selection and names its endpoints, instead of a row of hashes", async ({ page }) => {
  await openCompare(page);

  await expect(head(page).locator("h2")).toHaveText("Comparing 6 commits");
  // what the compare actually produced, next to the title
  await expect(head(page).locator(".cmp-stats")).toContainText("3 files");
  await expect(head(page).locator(".cmp-stats")).toContainText("+16");
  await expect(head(page).locator(".cmp-stats")).toContainText("−3");

  // oldest first, newest second — commit TIME order, not the order the graph
  // handed over (which is newest first)
  const ends = head(page).locator(".cmp-ends .cmp-end");
  await expect(ends).toHaveCount(2);
  await expect(ends.nth(0)).toContainText("commit subject 5");
  await expect(ends.nth(1)).toContainText("commit subject 0");

  // and no chip spill: six selected commits, two chips on screen
  await expect(head(page).locator("app-sha-chip")).toHaveCount(2);
});

test("the rest of the selection hides behind one disclosure, and that list scrolls", async ({ page }) => {
  // enough commits that the open list would otherwise push the file list off
  // the bottom of the pane
  await openCompare(page, 14);

  const toggle = head(page).locator(".cmp-more");
  await expect(toggle).toHaveText(/show all 14 commits/);
  await expect(head(page).locator(".cmp-list")).toHaveCount(0);

  await toggle.click();
  const list = head(page).locator(".cmp-list");
  await expect(list.locator(".cmp-end")).toHaveCount(14);
  await expect(list.locator(".cmp-end").first()).toContainText("commit subject 13");

  // a long selection must not grow the header: the list is capped and scrolls
  const scroll = await list.evaluate((el) => ({
    client: el.clientHeight,
    content: el.scrollHeight,
    overflow: getComputedStyle(el).overflowY,
  }));
  expect(scroll.client).toBeLessThanOrEqual(220);
  expect(scroll.content).toBeGreaterThan(scroll.client);
  expect(["auto", "scroll"]).toContain(scroll.overflow);

  await toggle.click();
  await expect(head(page).locator(".cmp-list")).toHaveCount(0);
});

test("a commit row in the header opens that commit's own diff", async ({ page }) => {
  await openCompare(page);
  await head(page).locator(".cmp-ends .cmp-end").first().click();

  await expect(page.locator("app-commit-diff-view")).toBeVisible();
  await expect(page.locator("app-range-diff-view")).toHaveCount(0);
});

test("the file list is the shared one: folders collapse", async ({ page }) => {
  await openCompare(page);

  const list = page.locator("app-range-diff-view app-diff-file-list");
  await expect(list.locator(".diff-dir", { hasText: "deep" })).toBeVisible();
  await expect(list.locator(".diff-file", { hasText: "two.ts" })).toBeVisible();

  // collapsing src takes its whole subtree off screen — the compare view had
  // no chevrons at all before
  await list.locator(".diff-dir", { hasText: "src" }).first().click();
  await expect(list.locator(".diff-file", { hasText: "two.ts" })).toHaveCount(0);
  await expect(list.locator(".diff-file", { hasText: "README.md" })).toBeVisible();

  await list.locator(".diff-dir", { hasText: "src" }).first().click();
  await expect(list.locator(".diff-file", { hasText: "two.ts" })).toBeVisible();
});

test("the file column resizes, on the same width as the working-changes list", async ({ page }) => {
  await openCompare(page);

  const columns = () =>
    page
      .locator("app-range-diff-view .diff-grid")
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns);
  expect(await columns()).toMatch(/^300px /);

  // drag the shared handle
  const handle = page.locator("app-range-diff-view app-pane-resizer");
  const box = (await handle.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 90, box.y + box.height / 2, { steps: 8 });
  await page.mouse.up();
  expect(await columns()).toMatch(/^39[0-9]px /);

  // ONE width for every diff surface: back on working changes, the same column
  await page.evaluate(ui(`.setGitView("${AGENT}", null)`));
  await expect(page.locator("app-diff-view")).toBeVisible();
  const working = await page
    .locator("app-diff-view .diff-grid")
    .evaluate((el) => getComputedStyle(el).gridTemplateColumns);
  expect(working).toMatch(/^39[0-9]px /);
});
