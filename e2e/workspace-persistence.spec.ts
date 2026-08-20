import { expect, Page, test } from "@playwright/test";

/**
 * E2E for workspace persistence: UiStore saves the layout (tabs, active tab,
 * scope, pane trees) to localStorage on every change and restores it on boot —
 * so a quit, crash, or update relaunch all come back to the same workspace.
 * The update path additionally records the running agents' ids (one-shot),
 * surfaced as `updateResumeIds` for the terminal-resume flow.
 *
 * Backend-free: a page reload is exactly what a relaunch does to the webview.
 */

const seedAgent = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "p-e2e", tool: "claude", model: "m", name: "${name}",
    task: "", status: "idle", branch: "agent/${name}", worktree: "", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

const uiState = `(() => {
  const u = window.ng.getComponent(document.querySelector("app-top-bar")).ui;
  return {
    tabs: u.tabs(),
    active: u.activeTab(),
    scope: u.scopeAgentId(),
    resume: u.updateResumeIds,
    roots: u.paneRoots(),
  };
})()`;

type UiState = {
  tabs: { id: string; kind: string }[];
  active: string;
  scope: string | null;
  resume: string[];
  roots: Record<string, { type: string; agentId: string | null; view: string }>;
};

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
}

test("the workspace layout survives a relaunch without any explicit snapshot", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedAgent("e2e-ws1", "e2e-workspace"));
  await page.evaluate(ui(`.openAgent("e2e-ws1")`));

  const before = (await page.evaluate(uiState)) as UiState;
  const agentTab = before.tabs.find((t) => t.kind === "agent");
  expect(agentTab).toBeTruthy();

  // a quit / crash / update relaunch, as seen by the webview
  await page.reload();
  await page.waitForSelector("app-top-bar");

  const after = (await page.evaluate(uiState)) as UiState;
  expect(after.tabs.map((t) => t.id)).toEqual(before.tabs.map((t) => t.id));
  expect(after.active).toBe(agentTab!.id);
  expect(after.scope).toBe("e2e-ws1");
  expect(after.roots[agentTab!.id]).toMatchObject({
    type: "leaf",
    agentId: "e2e-ws1",
    view: "terminal",
  });

  // fresh tab ids must not collide with the restored ones
  await page.evaluate(ui(`.openTicketDraft()`));
  const withDraft = (await page.evaluate(uiState)) as UiState;
  const ids = withDraft.tabs.map((t) => t.id);
  expect(new Set(ids).size).toBe(ids.length);

  // closing the restored tab persists too — the next boot must not resurrect it
  await page.evaluate(ui(`.closeTab("${agentTab!.id}")`));
  await page.reload();
  await page.waitForSelector("app-top-bar");
  const closed = (await page.evaluate(uiState)) as UiState;
  expect(closed.tabs.some((t) => t.id === agentTab!.id)).toBe(false);
});

test("the update-resume list is drained one-shot into updateResumeIds", async ({ page }) => {
  await boot(page);
  // what SettingsStore.install() writes right before UpdateInstall
  await page.evaluate(
    `localStorage.setItem("orrery:update-restore", JSON.stringify({ v: 1, at: Date.now(), resume: ["e2e-ws2"] }))`,
  );

  await page.reload();
  await page.waitForSelector("app-top-bar");
  expect(((await page.evaluate(uiState)) as UiState).resume).toEqual(["e2e-ws2"]);

  // one-shot: a normal restart later must not resume anything
  await page.reload();
  await page.waitForSelector("app-top-bar");
  expect(((await page.evaluate(uiState)) as UiState).resume).toEqual([]);
});
