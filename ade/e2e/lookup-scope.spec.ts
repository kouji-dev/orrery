import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the shared LOOKUP SCOPE — the answer to "I can't tell where the app is
 * looking". Ctrl+Shift+F (find in files) and Search Everywhere both render the
 * same <app-scope-bar> naming the project + worktree they read from, and share
 * ONE root store, so re-pointing the scope in one overlay is still in force
 * after it is closed and another is opened.
 *
 * Also covers the Symbols lookup (Ctrl+T), which is deliberately distinct from
 * the Files lookup: its own tab, its own 2-character floor, its own empty copy.
 *
 * Backend-free: every invoke rejects, so the specs assert chrome, seeded-store
 * data and honest empty states — never fabricated search results.
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-ls", name: "ls-proj", path: "C:/ls", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main", branches: ["main"],
  });
})()`;

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-ls", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "C:/wt/${name}", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

/**
 * Drop focus out of the agent tab's terminal. Plain Ctrl chords are NOT on the
 * terminal steal list — they flow to the PTY untouched, by design — so a spec
 * that opens an agent tab must leave the xterm helper textarea before pressing
 * Ctrl+E / Ctrl+T, exactly as a user clicking elsewhere would.
 */
async function blur(page: Page): Promise<void> {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent("ls-a1", "alpha"));
  await page.evaluate(seedAgent("ls-a2", "beta"));
  await page.evaluate(ui(`.openAgent("ls-a1")`));
}

/** The scope bar's worktree select is the second one (project, worktree, kind). */
const worktreeSelect = (root: string) =>
  `${root} app-scope-bar kj-select >> nth=1`;

test("every lookup overlay shows the scope it is reading", async ({ page }) => {
  await boot(page);

  // Ctrl+Shift+F — the badge spells the context out instead of "this worktree"
  await blur(page);
  await page.keyboard.press("Control+Shift+F");
  const find = page.locator("app-find-in-files");
  await expect(find.locator("app-scope-bar")).toBeVisible();
  await expect(find.locator("kj-badge").first()).toContainText("grep · ls-proj");
  await page.keyboard.press("Escape");
  await expect(find).toHaveCount(0);

  // Search Everywhere — same bar, with the worktree/project/all kind select
  await page.keyboard.press("Control+K");
  const se = page.locator("app-search-everywhere");
  await expect(se.locator("app-scope-bar kj-select")).toHaveCount(3);
  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});

test("re-pointing the scope in one overlay survives into the next", async ({ page }) => {
  await boot(page);

  await page.keyboard.press("Control+Shift+F");
  const find = page.locator("app-find-in-files");
  await expect(find.locator("kj-badge").first()).toContainText("alpha");

  // re-point the worktree — the overlays share ONE root store
  await page.locator(worktreeSelect("app-find-in-files")).locator(".kj-select-trigger").click();
  await page.getByRole("option", { name: "beta", exact: true }).click();
  await expect(find.locator("kj-badge").first()).toContainText("beta");
  await page.keyboard.press("Escape");

  // …and it is still beta after the overlay was destroyed and a DIFFERENT one opened
  await blur(page);
  await page.keyboard.press("Control+K");
  await expect(page.locator("app-search-everywhere app-scope-bar")).toContainText("beta");
});

test("Ctrl+T opens the Symbols lookup, distinct from the Files lookup", async ({ page }) => {
  await boot(page);
  await blur(page);
  await page.keyboard.press("Control+T");

  const se = page.locator("app-search-everywhere");
  const input = se.locator("input");
  await expect(input).toBeFocused();

  // symbols is its own corpus with its own empty copy — not the file lookup
  await expect(se).toContainText("start typing to search symbols");
  await expect(se.getByText("Symbols", { exact: true })).toBeVisible();

  // a 1-char query is accepted: symbols are one index lookup now, not a grep
  // that a short query would truncate (the old "type at least 2 characters"
  // refusal went with the grep)
  await input.fill("h");
  await expect(se).not.toContainText("start typing to search symbols");

  // Files is a separate tab with its own copy
  await input.fill("");
  await se.getByText("Files", { exact: true }).click();
  await expect(se).toContainText("start typing to search files");

  await page.keyboard.press("Escape");
  await expect(se).toHaveCount(0);
});

test("Go to Symbol is a real command, distinct from Go to File", async ({ page }) => {
  await boot(page);
  await blur(page);
  await page.keyboard.press("Control+Shift+P");
  // portalled panel again: input and rows are addressed globally
  const paletteInput = page.locator(".kj-command-palette__input");
  await expect(paletteInput).toBeFocused();
  await paletteInput.fill("Go to");
  await expect(page.getByText("Go to Symbol…")).toBeVisible();
  await expect(page.getByText("Go to File…")).toBeVisible();
});
