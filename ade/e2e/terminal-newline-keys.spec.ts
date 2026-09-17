import { expect, Page, test } from "@playwright/test";

/**
 * E2E for terminal newline keys: Shift+Enter and Ctrl+Enter must insert a
 * line break in the agent CLI's composer (backslash+CR — the sequence Claude
 * Code's own /terminal-setup binds), NOT submit the message. Plain Enter
 * still sends CR (submit).
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

/** Record every invoke; serve the terminal-recovery snapshot with empty text. */
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
  await page.evaluate(seedAgent("e2e-tk1", "e2e-termkeys"));
  await page.evaluate(stubBridge());
  await page.evaluate(ui(`.openAgent("e2e-tk1", "terminal")`));
  // xterm is mounted once its hidden helper textarea exists; typing goes there
  await page.waitForSelector(".xterm-helper-textarea", { state: "attached" });
  await page.locator(".xterm-helper-textarea").focus();
}

/** All agent_input payloads recorded so far, in order. */
const sentInputs = (page: Page) =>
  page.evaluate(
    `window.__calls.filter(c => c.command === "agent_input").map(c => c.args.data)`,
  ) as Promise<string[]>;

test("Shift+Enter and Ctrl+Enter insert a newline instead of submitting", async ({ page }) => {
  await openTerminal(page);

  await page.keyboard.press("Shift+Enter");
  await page.keyboard.press("Control+Enter");

  expect(await sentInputs(page)).toEqual(["\\\r", "\\\r"]);
});

test("plain Enter still submits (sends a bare CR)", async ({ page }) => {
  await openTerminal(page);

  await page.keyboard.press("Enter");

  expect(await sentInputs(page)).toEqual(["\r"]);
});
