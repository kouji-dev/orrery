import { expect, Page, test } from "@playwright/test";

/**
 * E2E for Annotate in the file tab.
 *
 * The blame used to replace the file with a hand-rolled surface; it now rides
 * along IN the Monaco editor as an injected column, so these tests assert the
 * two halves of that: the blame is there, and the file is still there — real
 * editor, real scrolling, real content.
 *
 * Backend-free: `agent_diff` is stubbed on AgentsStore, `agent_working_blame`
 * on the shared bridge. Everything else rejects, as in the other file specs.
 */

const FILE = "src/app.ts";
const LINES = Array.from({ length: 120 }, (_, i) => `const v${i} = ${i};`);
const TEXT = LINES.join("\n") + "\n";

const seedAgent = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "e2e-blame", projectId: "p-e2e", tool: "claude", model: "m", name: "e2e-blamer",
    task: "", status: "idle", branch: "agent/blame", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Stub the working-tree read + the blame: two commits and one dirty line. */
const seedBlame = (text: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.agentActions["agentsStore"];
  store.diff = () => Promise.resolve({ old: "", new: ${JSON.stringify(text)} });
  const bridge = store["bridge"];
  const orig = bridge.invoke.bind(bridge);
  const now = Math.floor(Date.now() / 1000);
  const lines = ${JSON.stringify(LINES)}.map((s, i) => ({
    n: i + 1,
    c: i === 0 ? 2 : i < 60 ? 0 : 1,
    line: s,
  }));
  bridge.invoke = (cmd, args) => {
    if (cmd === "agent_working_blame") {
      return Promise.resolve({
        old: { commits: [], lines: [] },
        new: {
          commits: [
            { sha: "aaa1111bbb", author: "Ada Lovelace", when: now - 3600, summary: "first pass" },
            { sha: "ccc3333ddd", author: "Grace Hopper", when: now - 86400 * 3, summary: "second pass" },
            { sha: "0000000", author: "Uncommitted", when: 0, summary: "Uncommitted changes" },
          ],
          lines,
        },
      });
    }
    return orig(cmd, args);
  };
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openFile(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent);
  await page.evaluate(seedBlame(TEXT));
  await page.evaluate(ui(`.openFileInWorkspace("e2e-blame", "${FILE}")`));
  await expect(page.locator("app-file-view")).toBeVisible();
  // Monaco arrives as a lazy chunk
  await expect(page.locator("app-monaco-file-editor .monaco-editor")).toBeVisible({ timeout: 30_000 });
}

test("Annotate keeps the file in the editor and adds a blame column", async ({ page }) => {
  await openFile(page);
  await page.getByRole("button", { name: "Annotate", exact: true }).click();

  // the blame column is injected text inside the editor…
  const blame = page.locator("app-monaco-file-editor .blm");
  await expect(blame.first()).toBeVisible({ timeout: 15_000 });
  // …carrying initials + short sha of the run's first line
  await expect(blame.filter({ hasText: "aaa1111" }).first()).toBeVisible();
  await expect(blame.filter({ hasText: "AL" }).first()).toBeVisible();

  // …and the FILE is still the file: same editor, real content, no takeover
  await expect(page.locator("app-monaco-file-editor .monaco-editor")).toBeVisible();
  await expect(page.locator("app-annotate-blame")).toHaveCount(0);
  await expect(page.locator("app-monaco-file-editor")).toContainText("const v0 = 0;");

  // toggling Annotate off leaves the editor untouched and drops the column
  await page.getByRole("button", { name: "Annotate", exact: true }).click();
  await expect(page.locator("app-monaco-file-editor .blm")).toHaveCount(0);
  await expect(page.locator("app-monaco-file-editor")).toContainText("const v0 = 0;");
});

test("the annotated file still scrolls", async ({ page }) => {
  await openFile(page);
  await page.getByRole("button", { name: "Annotate", exact: true }).click();
  await expect(page.locator("app-monaco-file-editor .blm").first()).toBeVisible({ timeout: 15_000 });

  // Monaco owns the scrolling. It translates its lines rather than letting the
  // DOM scroll, so the editor's own scroll position is what to read.
  const top = async () =>
    page.evaluate(() => {
      const ed = (window as any).ng.getComponent(document.querySelector("app-monaco-file-editor"));
      return Math.round(ed?.["editor"]?.getScrollTop?.() ?? -1);
    });
  const height = await page.evaluate(() => {
    const ed = (window as any).ng.getComponent(document.querySelector("app-monaco-file-editor"));
    const e = ed?.["editor"];
    return { total: e?.getScrollHeight?.() ?? 0, view: e?.getLayoutInfo?.().height ?? 0 };
  });
  expect(height.total).toBeGreaterThan(height.view); // 120 lines do not fit
  expect(await top()).toBe(0);
  await page.locator("app-monaco-file-editor .monaco-editor").hover();
  await page.mouse.wheel(0, 600);
  await expect.poll(top).toBeGreaterThan(0);
});

test("a failed blame reports itself and still shows the file", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent);
  // only the file read is stubbed — the blame invoke rejects like every other
  await page.evaluate(`(() => {
    const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
    bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: ${JSON.stringify(TEXT)} });
  })()`);
  await page.evaluate(ui(`.openFileInWorkspace("e2e-blame", "${FILE}")`));
  await expect(page.locator("app-monaco-file-editor .monaco-editor")).toBeVisible({ timeout: 30_000 });

  await page.getByRole("button", { name: "Annotate", exact: true }).click();
  await expect(page.locator("app-file-view .blame-strip")).toContainText("blame failed:");
  // the failure is a strip, not a blank pane — the file is still readable
  await expect(page.locator("app-monaco-file-editor")).toContainText("const v0 = 0;");
});
