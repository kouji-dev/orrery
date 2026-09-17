import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the M3 footer: the aggregate language-server chip, its popover
 * (rows grouped by project, Stop / Stop all), and the live instance sub-rows
 * under a server pack in the Extensions modal.
 *
 * Backend-free: the shared bridge is stubbed after boot — `on` records its
 * handlers so the test pushes `lsp://status` like the backend would, `lsp_*`
 * and `ext_*` invokes are recorded and answered canned — and both stores
 * re-attach through `connect()` (the extensions-panel spec pattern).
 */

const MB = 1024 * 1024;

const inst = (over: Record<string, unknown>) => ({
  extId: "server.jdtls",
  label: "jdtls",
  language: "java",
  root: "C:/p",
  projectId: "p-e2e",
  projectName: "e2e-proj",
  pid: 4242,
  state: "ready",
  memBytes: 812 * MB,
  cpu: 1.5,
  restarts: 0,
  startedAt: Date.now() - 65_000,
  lastError: null,
  ...over,
});

const ONE = [inst({ id: "server.jdtls:p-e2e" })];
const THREE = [
  inst({ id: "server.jdtls:p-e2e", memBytes: 1024 * MB }),
  inst({ id: "server.gopls:p-e2e", extId: "server.gopls", label: "gopls", language: "go", memBytes: 102.4 * MB, pid: 4243 }),
  inst({ id: "server.jdtls:p-two", projectId: "p-two", projectName: "second", memBytes: 1024 * MB, pid: 4244 }),
];

const pack = (over: Record<string, unknown>) => ({
  kind: "server",
  description: "",
  version: "1.0.0",
  installedVersion: "1.0.0",
  languages: ["java"],
  sizeBytes: 40 * MB,
  installed: true,
  enabled: true,
  compatible: true,
  availableForTarget: true,
  state: "installed",
  error: null,
  detection: { status: "found", path: "/opt/jdtls/bin/jdtls", hint: null },
  ...over,
});

const VIEW = {
  registryUrl: "https://example.test/index.json",
  fetchedAt: Date.now() - 120_000,
  offline: false,
  items: [
    pack({ id: "server.jdtls", name: "Eclipse JDT Language Server" }),
    pack({ id: "server.gopls", name: "gopls", languages: ["go"] }),
  ],
};

/** Stub `lsp_*` + `ext_*` on the shared bridge, record `on` handlers, re-attach both stores. */
const seed = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const sb = window.ng.getComponent(document.querySelector("app-status-bar"));
  const bridge = bar.extensions["bridge"];
  const origInvoke = bridge.invoke.bind(bridge);
  const origOn = bridge.on.bind(bridge);
  const rec = (window.__lsp = { calls: [], handlers: {} });
  bridge.invoke = (cmd, args) => {
    if (cmd.startsWith("lsp_") || cmd.startsWith("ext_")) {
      rec.calls.push([cmd, args]);
      if (cmd === "lsp_status") return Promise.resolve({ servers: [] });
      if (cmd === "ext_registry_list") return Promise.resolve(${JSON.stringify(VIEW)});
      return Promise.resolve(null);
    }
    return origInvoke(cmd, args);
  };
  bridge.on = (event, handler) => {
    if (event === "lsp://status" || event.startsWith("ext://")) {
      (rec.handlers[event] ||= []).push(handler);
      return Promise.resolve(() => {});
    }
    return origOn(event, handler);
  };
  bar.extensions.connect();
  sb.lsp.connect();
})()`;

const emit = (servers: unknown[]) =>
  `(window.__lsp.handlers["lsp://status"] || []).forEach((h) => h(${JSON.stringify({ servers })}))`;
const calls = (page: Page, cmd: string) =>
  page.evaluate(`window.__lsp.calls.filter(([c]) => c === ${JSON.stringify(cmd)}).map(([, a]) => a)`);

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-status-bar");
  await page.evaluate(seed);
  // the seed answered "no servers" — the marker is permanent, so it is there
  // in its quietest form rather than absent (a vanished marker would hide a
  // crash or a server that never came up)
  await expect(page.locator("[data-testid=lsp-chip]")).toHaveText("no servers");
}

const chip = (page: Page) => page.locator("app-status-bar [data-testid=lsp-chip]");
const pop = (page: Page) => page.locator("[data-testid=lsp-popover]");

test("any live instance → 'servers' (name and memory stay in the popover); tints follow the states; empty → 'no servers'", async ({ page }) => {
  await boot(page);
  await page.evaluate(emit(ONE));
  await expect(chip(page)).toBeVisible();
  await expect(chip(page)).toHaveText("servers");
  await expect(chip(page)).toHaveClass(/running/);

  await page.evaluate(emit(THREE));
  await expect(chip(page)).toHaveText("servers");
  await expect(chip(page)).toHaveAttribute("title", "jdtls · e2e-proj\ngopls · e2e-proj\njdtls · second");

  // any starting → spinner; any crashed → tint + count; all idle → dimmed
  await page.evaluate(emit([inst({ id: "server.jdtls:p-e2e", state: "starting", memBytes: 0, startedAt: null, pid: null })]));
  await expect(chip(page).locator("kj-spinner")).toBeVisible();
  await expect(chip(page)).toHaveClass(/starting/);
  await page.evaluate(emit([...ONE, inst({ id: "server.gopls:p-e2e", extId: "server.gopls", label: "gopls", state: "crashed", memBytes: 0, lastError: "exit code 1\njava.lang.OutOfMemoryError" })]));
  await expect(chip(page)).toHaveClass(/error/);
  await expect(chip(page)).toHaveText("servers");
  await page.evaluate(emit([inst({ id: "server.jdtls:p-e2e", state: "idle" })]));
  await expect(chip(page)).toHaveClass(/idle/);

  await page.evaluate(emit([]));
  await expect(chip(page)).toHaveClass(/none/);
  await expect(chip(page)).toHaveText("no servers");
});

test("a server that could not be launched is listed as 'not found' with its reason, not hidden", async ({ page }) => {
  await boot(page);
  // nothing runs, but the chip must not read as "no servers" and nothing else
  await page.evaluate(
    emit([inst({ id: "server.jdtls:p-e2e", state: "missing", pid: null, memBytes: 0, startedAt: null, lastError: "runtime.java is not installed — reinstall server.jdtls from Extensions" })]),
  );
  await expect(chip(page)).toHaveClass(/error/);
  await expect(chip(page)).toHaveText("no servers");
  await chip(page).click();
  await expect(pop(page)).toBeVisible();
  await expect(page.locator("[data-testid=lsp-empty]")).toHaveCount(0);
  const row = pop(page).locator(".lsp-row.err");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("not found");
  await expect(row.locator(".lsp-err")).toContainText("runtime.java is not installed");
  // nothing to stop; Restart forces a fresh launch attempt
  await expect(row.getByRole("button", { name: "Stop" })).toHaveCount(0);
  await row.hover();
  await row.getByRole("button", { name: "Restart" }).click();
  await expect.poll(() => calls(page, "lsp_restart")).toEqual([{ extId: "server.jdtls", projectId: "p-e2e" }]);
  // a manual stop stays listed too, quietly (no error tint, no count)
  await page.evaluate(emit([inst({ id: "server.jdtls:p-e2e", state: "stopped", pid: null, memBytes: 0, startedAt: null })]));
  await expect(chip(page)).toHaveClass(/none/);
  await expect(chip(page)).toHaveText("no servers");
  await expect(pop(page).locator(".lsp-row.dim")).toContainText("stopped");
});

test("with nothing running the marker still opens and explains itself", async ({ page }) => {
  await boot(page);
  // permanent marker, quiet state
  await expect(chip(page)).toHaveText("no servers");
  await expect(chip(page)).toHaveClass(/none/);

  // and it is still a way IN: the popover opens and says why it is empty
  await chip(page).click();
  await expect(pop(page)).toBeVisible();
  await expect(page.locator("[data-testid=lsp-empty]")).toBeVisible();
  await expect(pop(page)).toContainText(/No language server/);
  // the popover keeps its footer route into Extensions
  await expect(pop(page).getByRole("button", { name: "Open extensions" })).toBeVisible();
  // nothing to stop, so the header offers nothing to stop
  await expect(pop(page).getByRole("button", { name: "Stop all" })).toHaveCount(0);
});

test("popover: rows grouped by project, Stop → lsp_stop, Stop all → lsp_stop_all, crashed row shows its error", async ({ page }) => {
  await boot(page);
  await page.evaluate(emit(THREE));
  await chip(page).click();
  await expect(pop(page)).toBeVisible();
  await expect(pop(page).locator(".lsp-pop-h")).toContainText("Language servers");
  await expect(pop(page).locator(".lsp-pop-h .tot")).toHaveText("3 · 2.10 GB");

  const groups = pop(page).locator(".lsp-g");
  await expect(groups).toHaveCount(2);
  await expect(groups.nth(0)).toContainText("e2e-proj");
  await expect(groups.nth(0).locator(".n")).toHaveText("2");
  await expect(groups.nth(1)).toContainText("second");
  const rows = pop(page).locator(".lsp-row");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0).locator(".nm")).toHaveText("jdtls");
  await expect(rows.nth(0)).toContainText("running");
  await expect(rows.nth(0).locator(".fig").first()).toHaveText("1.00 GB");
  await expect(rows.nth(1).locator(".nm")).toHaveText("gopls");
  // uptime ticks while the popover is open
  await expect(rows.nth(0).locator(".fig.sub")).toContainText(/(\d+m )?\d+s$/);

  // Stop on the second row (hover reveals the actions)
  await rows.nth(1).hover();
  await rows.nth(1).getByRole("button", { name: "Stop" }).click();
  await expect.poll(() => calls(page, "lsp_stop")).toEqual([{ extId: "server.gopls", projectId: "p-e2e" }]);
  await rows.nth(2).hover();
  await rows.nth(2).getByRole("button", { name: "Restart" }).click();
  await expect.poll(() => calls(page, "lsp_restart")).toEqual([{ extId: "server.jdtls", projectId: "p-two" }]);

  // a crashed instance expands its last error in place
  await page.evaluate(emit([...THREE.slice(0, 1), inst({ id: "server.gopls:p-e2e", extId: "server.gopls", label: "gopls", state: "crashed", memBytes: 0, lastError: "exit code 1\njava.lang.OutOfMemoryError" })]));
  const crashed = pop(page).locator(".lsp-row.err");
  await expect(crashed).toHaveCount(1);
  await expect(crashed.locator(".lsp-err")).toContainText("OutOfMemoryError");
  await expect(crashed.getByRole("button", { name: "Stop" })).toHaveCount(0);

  // Stop all closes the popover and asks the backend
  await pop(page).getByRole("button", { name: "Stop all" }).click();
  await expect.poll(() => calls(page, "lsp_stop_all")).toEqual([{}]);
  // kouji keeps a closed panel in the DOM, hidden
  await expect(pop(page)).toBeHidden();

  // the footer link opens the Extensions modal on the servers section
  await chip(page).click();
  await pop(page).getByRole("button", { name: "Open extensions" }).click();
  const dialog = page.locator("app-extensions-modal[role=dialog]");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".set-head .ht")).toHaveText("Language servers");
});

test("Extensions modal: a server row lists its live instances and the nav foot counts them", async ({ page }) => {
  await boot(page);
  await page.evaluate(emit(THREE));
  await page.keyboard.press("Control+Shift+X");
  const dialog = page.locator("app-extensions-modal[role=dialog]");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".set-nav-foot")).toContainText("3 running");
  await dialog.locator(".set-nav-item").filter({ hasText: "Language servers" }).click();

  const row = dialog.locator('.ext-row[data-ext-id="server.jdtls"]');
  await expect(row.locator(".ext-up")).toHaveText("2 instances");
  const insts = row.locator(".ext-inst");
  await expect(insts).toHaveCount(2);
  await expect(insts.nth(0).locator(".pj")).toHaveText("e2e-proj");
  await expect(insts.nth(0)).toContainText("running");
  await expect(insts.nth(0)).toContainText("1.0 GB");
  await expect(insts.nth(0)).toContainText(/up (\d+m )?\d+s/);
  await expect(insts.nth(1).locator(".pj")).toHaveText("second");
  // ≥ 2 instances → "Stop all" on the row; a single Stop on a sub-row
  await insts.nth(1).getByRole("button", { name: "Stop", exact: true }).click();
  await expect.poll(() => calls(page, "lsp_stop")).toEqual([{ extId: "server.jdtls", projectId: "p-two" }]);
  await row.getByRole("button", { name: "Stop all" }).click();
  await expect.poll(() => calls(page, "lsp_stop")).toHaveLength(3);

  // gopls has one instance: no Stop all on the row
  const go = dialog.locator('.ext-row[data-ext-id="server.gopls"]');
  await expect(go.locator(".ext-inst")).toHaveCount(1);
  await expect(go.getByRole("button", { name: "Stop all" })).toHaveCount(0);

  // past three instances the list collapses to a summary that expands
  await page.evaluate(
    emit([
      ...THREE,
      inst({ id: "server.jdtls:p-3", projectId: "p-3", projectName: "third", state: "idle", memBytes: 100 * MB }),
      inst({ id: "server.jdtls:p-4", projectId: "p-4", projectName: "fourth", state: "crashed", memBytes: 0, lastError: "boom" }),
    ]),
  );
  await expect(row.locator(".ext-up")).toHaveText("4 instances");
  const sum = row.locator(".ext-inst.sum");
  await expect(sum).toHaveText(/2 running · 1 idle · 1 error · 2.1 GB/);
  await sum.click();
  await expect(row.locator(".ext-inst:not(.sum)")).toHaveCount(4);
  await expect(row.locator(".ext-inst.err .ext-line.err")).toContainText("boom");
  await row.locator(".ext-inst.sum").click();
  await expect(row.locator(".ext-inst:not(.sum)")).toHaveCount(0);
});
