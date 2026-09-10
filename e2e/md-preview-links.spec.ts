import { expect, Page, test } from "@playwright/test";

/**
 * E2E for links inside the rendered markdown preview. A markdown link is a real
 * <a href> inside [innerHTML]; left alone the webview navigates to it and the
 * whole app unloads. Every link is intercepted: `#anchor` scrolls within the
 * document, a relative path opens that file as another tab in the same
 * worktree, and an external URL is handed to the OS browser — never navigated.
 *
 * Backend-free: the file read is stubbed; the opener plugin is absent in the
 * browser build, so the external case asserts "no navigation + a notice".
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const filler = Array.from({ length: 60 }, (_, i) => `Paragraph ${i} of filler text so the page scrolls.`).join("\n\n");
const MD = [
  "# Links",
  "",
  "See the [design notes](./notes/design.md), jump to [the end](#second-part),",
  "or read the [upstream docs](https://example.com/docs).",
  "",
  filler,
  "",
  "## Second part",
  "",
  "You made it.",
  "",
].join("\n");

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openMd(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-lk1", "e2e-links"));
  await page.evaluate(`(() => {
    const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
    bar.agentActions["agentsStore"].diff = () => Promise.resolve({ old: "", new: ${JSON.stringify(MD)} });
  })()`);
  await page.evaluate(ui(`.openFileInWorkspace("e2e-lk1", "docs/guide/index.md")`));
  await expect(page.locator(".md-body h1", { hasText: "Links" })).toBeVisible();
}

test("an external link never navigates the app away", async ({ page }) => {
  await openMd(page);
  const before = page.url();
  await page.locator(".md-body a", { hasText: "upstream docs" }).click();
  // still the app, same URL, and the user was told what happened
  await expect(page.locator("app-top-bar")).toBeVisible();
  expect(page.url()).toBe(before);
  await expect(page.getByText(/opened in browser|could not open link/)).toBeVisible();
  await expect(page.locator(".md-body h1", { hasText: "Links" })).toBeVisible();
});

test("a relative link opens that file, resolved against the current document's folder", async ({ page }) => {
  await openMd(page);
  await page.locator(".md-body a", { hasText: "design notes" }).click();
  // ./notes/design.md relative to docs/guide/index.md → docs/guide/notes/design.md
  const opened = await page.evaluate(() => {
    const bar = (window as any).ng.getComponent(document.querySelector("app-top-bar"));
    const roots = bar.ui.paneRoots();
    const files: string[] = [];
    const walk = (n: any) => { if (!n) return; if (n.type === "leaf") { files.push(...(n.files ?? []), n.activeFile ?? ""); return; } walk(n.a); walk(n.b); };
    Object.values(roots).forEach(walk);
    return files;
  });
  expect(opened).toContain("docs/guide/notes/design.md");
  await expect(page.locator("app-top-bar")).toBeVisible();
});

test("an anchor link scrolls to the matching heading inside the preview", async ({ page }) => {
  await openMd(page);
  const scroller = page.locator(".md-body");
  const heading = page.locator(".md-body h2", { hasText: "Second part" });
  const offscreen = await heading.evaluate((h, s) => h.getBoundingClientRect().top > (s as HTMLElement).getBoundingClientRect().bottom, await scroller.elementHandle());
  expect(offscreen).toBe(true);

  await page.locator(".md-body a", { hasText: "the end" }).click();
  await expect.poll(async () =>
    heading.evaluate((h, s) => {
      const r = h.getBoundingClientRect(); const b = (s as HTMLElement).getBoundingClientRect();
      return r.top >= b.top - 2 && r.top < b.bottom;
    }, await scroller.elementHandle()),
  ).toBe(true);
  expect(page.url()).not.toContain("#");
});
