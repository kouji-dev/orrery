import { expect, Page, test } from "@playwright/test";

/**
 * E2E for nested overlay stacking (kouji-ui core ≥ 0.8.3): an overlay opened
 * from inside another overlay renders ABOVE it. Regression for the reported
 * bug — the scope selects in the lookup overlays opened their listbox BEHIND
 * the overlay, because its panel sat at a hardcoded z-index 1001 while every
 * popover/select panel sat at 1000. Search Everywhere (Ctrl+K) is the host
 * here: the hand-rolled OverlayShell with the three-select scope bar.
 *
 * The proof is a real click on an option: Playwright refuses to click an
 * element another one covers, so a passing click IS the stacking assertion.
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-ovl", name: "ovl-proj", path: "C:/ovl", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main", branches: ["main"],
  });
})()`;

const seedAgent = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "a-ovl", projectId: "p-ovl", tool: "claude", model: "m", name: "ovl-agent",
    task: "", status: "idle", branch: "agent/ovl", worktree: "C:/wt/ovl", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
  const store = bar.agentActions["agentsStore"];
  store.diff = (id, path) => Promise.resolve({ old: "", new: "export const x = 1;\\n", lang: "typescript" });
  const bridge = store["bridge"];
  const orig = bridge.invoke.bind(bridge);
  bridge.invoke = (cmd, args) =>
    cmd.startsWith("nav_") || cmd.startsWith("symbols_") || cmd.startsWith("lsp_") || cmd.startsWith("libsrc_") || cmd === "agent_file_hunks"
      ? Promise.resolve(cmd === "symbols_search" ? [] : cmd.startsWith("nav_") ? { source: "index", locations: [], lspState: null } : null)
      : orig(cmd, args);
})()`;

const OVERLAY = "app-search-everywhere";
const OVERLAY_INPUT = `${OVERLAY} input`;
const SCOPE_TRIGGER = ".kj-select-trigger";
/** The select's own panel. NOT `[role=listbox]`: the palette's command list
 *  carries that role too, so it would match while no select is open. */
const SELECT_PANEL = ".kj-select-content:visible";

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openSearchWithFiles(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(seedAgent);
  await page.evaluate(ui(`.openAgent("a-ovl")`));
  await page.evaluate(ui(`.openFileInWorkspace("a-ovl", "src/alpha.ts")`));
  await page.evaluate(ui(`.openFileInWorkspace("a-ovl", "src/beta.ts")`));
  // the editor arrives as a lazy chunk; the terminal must not hold the chord
  await expect(page.locator("app-file-view")).toBeVisible();
  await page.mouse.click(600, 400);
  await page.locator("body").press("Control+k");
  await expect(page.locator(OVERLAY_INPUT)).toBeVisible();
}

test("a scope select inside the Search Everywhere overlay opens ABOVE it", async ({ page }) => {
  test.slow(); // Monaco's lazy chunk + two overlays + a real click
  await openSearchWithFiles(page);

  const trigger = page.locator(SCOPE_TRIGGER).first();
  await expect(trigger).toBeVisible();
  await trigger.click();

  // the panel is portalled into the shared overlay container, in its own wrapper
  await expect(page.locator(SELECT_PANEL)).toBeVisible();


  // and the stack really did lift it: every kouji overlay gets its own
  // `.kj-overlay-wrapper`, and the select's must carry the highest level
  const z = await page.evaluate(`(() => {
    const wrappers = Array.from(document.querySelectorAll(".kj-overlay-wrapper"))
      .map((w) => ({ z: parseInt(getComputedStyle(w).zIndex, 10) || 0, select: !!w.querySelector(".kj-select-content") }));
    return { count: wrappers.length, top: Math.max(...wrappers.map((w) => w.z)), selectZ: Math.max(...wrappers.filter((w) => w.select).map((w) => w.z), 0) };
  })()`);
  expect(z.count).toBeGreaterThanOrEqual(1); // at least the select's wrapper
  expect(z.selectZ).toBe(z.top); // the select owns the top level

  // The wrapper level alone proved nothing: the wrappers are display:contents
  // (no box, no stacking context), so every PANEL competes in the container's
  // own context — and an unlayered `z-index: 1` on the panels let an
  // overlay's backdrop (z 1000) paint over the listbox while this assertion
  // stayed green. The hit test is the real proof: the point at the middle of
  // the listbox must resolve to the listbox, not to a scrim.
  const hit = await page.evaluate(`(() => {
    const panel = Array.from(document.querySelectorAll(".kj-select-content")).find((p) => p.getBoundingClientRect().height > 0);
    const r = panel.getBoundingClientRect();
    const el = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return { inside: panel.contains(el), tag: el ? el.className : null, panelZ: getComputedStyle(panel).zIndex };
  })()`);
  expect(hit.inside, `the point in the listbox hit ${hit.tag}`).toBe(true);
  expect(parseInt(hit.panelZ, 10)).toBe(z.top); // the panel itself reads the stack level
});

test("picking an option closes the select and leaves the overlay open", async ({ page }) => {
  // (The kouji-ui defect that made this a fixme — a value-changing option
  // click charged to a <kj-command-palette>'s backdrop — cannot bite here:
  // no scope select lives inside a kj palette any more.)
  await openSearchWithFiles(page);
  await page.locator(SCOPE_TRIGGER).nth(1).click(); // the worktree select — real choices
  await expect(page.locator(SELECT_PANEL)).toBeVisible();

  // forced: the overlay repositions every frame, so it never reports "stable"
  await page.getByRole("option", { name: "ovl-agent", exact: true }).click({ force: true });
  await expect(page.locator(OVERLAY_INPUT)).toBeVisible();
  await expect(page.locator(SELECT_PANEL)).toHaveCount(0);
});

test("Escape closes the inner select first, the overlay second", async ({ page }) => {
  await openSearchWithFiles(page);
  await page.locator(SCOPE_TRIGGER).first().click();
  await expect(page.locator(SELECT_PANEL)).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.locator(SELECT_PANEL)).toHaveCount(0);
  await expect(page.locator(OVERLAY_INPUT)).toBeVisible(); // still open

  await page.keyboard.press("Escape");
  await expect(page.locator(OVERLAY_INPUT)).toHaveCount(0);
});
