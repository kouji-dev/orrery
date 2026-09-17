import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the terminal/keymap contract (steal-list model, EXCLUSIVE delivery):
 * with the terminal focused, only steal-listed chords fire app-side — by
 * default Ctrl/Mod+Shift chords and the blessed Ctrl+K (Search Everywhere),
 * plus double-Shift (emits no bytes, so no PTY program can want it). Every
 * other chord is forwarded to the PTY untouched, because the program inside
 * (readline, Claude Code, any TUI) may bind it — one keypress must never
 * execute on both sides. The list is per-command overridable in Settings →
 * Keymap (settings.keymapTerminal).
 *
 * Backend-free: the shared BRIDGE singleton's `invoke` is swapped for a
 * recorder, so every keystroke the xterm forwards lands in window.__calls.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "working", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const stubBridge = () => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const bridge = bar.agentActions["agentsStore"]["bridge"];
  window.__calls = [];
  bridge.invoke = (command, args) => {
    window.__calls.push({ command, args });
    if (command === "runtime_snapshot") return Promise.resolve({ text: "", endSeq: 0 });
    return Promise.resolve();
  };
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function openTerminal(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedAgent("e2e-km1", "e2e-termmap"));
  await page.evaluate(stubBridge());
  await page.evaluate(ui(`.openAgent("e2e-km1", "terminal")`));
  await page.waitForSelector(".xterm-helper-textarea", { state: "attached" });
  await page.locator(".xterm-helper-textarea").focus();
}

const sentInputs = (page: Page) =>
  page.evaluate(
    `window.__calls.filter(c => c.command === "agent_input").map(c => c.args.data)`,
  ) as Promise<string[]>;

test("Ctrl+K opens Search Everywhere from inside the terminal", async ({ page }) => {
  await openTerminal(page);
  await page.keyboard.press("Control+K");
  await expect(page.locator("app-search-everywhere input")).toBeVisible();
  // the chord was intercepted — nothing was forwarded to the PTY
  expect(await sentInputs(page)).toEqual([]);
});

test("Ctrl+Shift chords keep working from inside the terminal", async ({ page }) => {
  await openTerminal(page);
  await page.keyboard.press("Control+Shift+G");
  await expect(page.locator("app-tool-window")).toBeVisible();
});

test("double-Shift opens Search Everywhere from inside the terminal (emits no bytes)", async ({ page }) => {
  await openTerminal(page);
  await page.keyboard.press("Shift");
  await page.keyboard.press("Shift");
  await expect(page.locator("app-search-everywhere input")).toBeVisible();
  expect(await sentInputs(page)).toEqual([]);
});

test("non-stolen chords (Ctrl+S) flow to the PTY; a keymapTerminal override steals them", async ({ page }) => {
  await openTerminal(page);
  await page.keyboard.press("Control+S");
  // XOFF byte reached the PTY, Save All did not run app-side
  expect(await sentInputs(page)).toEqual([""]);
  // user opts the command into the steal list → the same chord now fires app-side
  await page.evaluate(`window.ng.getComponent(document.querySelector("app-top-bar"))
    .commands["settings"].setKeymapTerminal("file.save", true)`);
  await page.keyboard.press("Control+S");
  expect(await sentInputs(page)).toEqual([""]); // nothing new forwarded
});

test("shell chords (Ctrl+C) are forwarded to the PTY, not intercepted", async ({ page }) => {
  await openTerminal(page);
  await page.keyboard.press("Control+C");
  // ETX reaches the shell; no overlay/panel hijacked the keystroke
  expect(await sentInputs(page)).toEqual([""]);
  await expect(page.locator("app-search-everywhere")).toHaveCount(0);
});
