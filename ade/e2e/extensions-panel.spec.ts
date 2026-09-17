import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the Extensions panel (M1): the top-bar puzzle button (immediately
 * left of the settings gear), the Ctrl+Shift+X chord, one row per pack state,
 * Install → `ext_install`, and a faked `ext://progress` tick filling the bar.
 *
 * Backend-free: the extensions store re-attaches (`connect()`) to a stubbed
 * bridge whose `on` records its handlers on `window.__ext.handlers`, so the
 * test can push status/progress like the backend would.
 */

const MB = 1024 * 1024;
const pack = (over: Record<string, unknown>) => ({
  kind: "grammar",
  description: "",
  version: "1.0.0",
  installedVersion: null,
  languages: ["x"],
  sizeBytes: 2.1 * MB,
  requires: [],
  bundledSizeBytes: 0,
  installed: false,
  enabled: false,
  compatible: true,
  availableForTarget: true,
  state: "available",
  error: null,
  detection: null,
  ...over,
});

const VIEW = {
  registryUrl: "https://example.test/index.json",
  fetchedAt: Date.now() - 120_000,
  offline: false,
  items: [
    pack({ id: "rust", name: "Rust grammar", languages: ["rs"], installed: true, enabled: true, installedVersion: "0.23.4", version: "0.24.1", state: "installed" }),
    pack({ id: "ts", name: "TypeScript & TSX", languages: ["ts", "tsx", "js", "jsx"], installed: true, enabled: true, installedVersion: "0.21.0", version: "0.21.0", state: "installed" }),
    pack({ id: "java", name: "Java grammar", languages: ["java"], installed: true, enabled: false, installedVersion: "0.19.2", version: "0.19.2", state: "installed" }),
    pack({ id: "cpp", name: "C / C++ bundle", languages: ["c", "cpp", "h", "hpp", "cc", "cmake"], sizeBytes: 40 * MB, state: "downloading" }),
    pack({ id: "kotlin", name: "Kotlin grammar", languages: ["kt", "kts"], state: "available" }),
    pack({ id: "go", name: "Go grammar", languages: ["go", "mod"], installed: true, enabled: true, installedVersion: "0.18.0", version: "0.18.0", state: "pendingRestart" }),
    pack({ id: "zig", name: "Zig grammar", languages: ["zig"], state: "error", error: "sha256 mismatch — archive does not match the registry manifest" }),
    pack({ id: "hs", name: "Haskell grammar", languages: ["hs", "lhs"], state: "incompatible", compatible: false, minAppVersion: "0.24" }),
    pack({ id: "jdtls", kind: "server", name: "Eclipse JDT Language Server", languages: ["java"], installed: true, enabled: true, installedVersion: "1.38.0", version: "1.38.0", state: "installed", detection: { status: "configured", path: "/opt/jdtls/bin/jdtls", hint: "needs JDK 17+ — JAVA_HOME not set" } }),
    pack({ id: "clangd", kind: "server", name: "clangd", languages: ["c", "cpp"], sizeBytes: 38 * MB, state: "available" }),
    // self-contained packs: a runtime, a server bundled on it, one still to install, one found on PATH only
    pack({ id: "runtime.node", kind: "runtime", name: "Node runtime 22", languages: [], sizeBytes: 46 * MB, installed: true, enabled: true, installedVersion: "22.4.0", version: "22.4.0", state: "installed" }),
    pack({ id: "pyright", kind: "server", name: "Pyright", languages: ["py"], requires: ["runtime.node"], installed: true, enabled: true, installedVersion: "1.1.400", version: "1.1.400", state: "installed", detection: { status: "bundled", path: "C:/packs/pyright/langserver.js", hint: null } }),
    pack({ id: "tsserver", kind: "server", name: "TypeScript language server", languages: ["ts", "tsx"], requires: ["runtime.node"], sizeBytes: 12 * MB, bundledSizeBytes: 58 * MB, state: "available" }),
    pack({ id: "gopls", kind: "server", name: "gopls", languages: ["go"], sizeBytes: 20 * MB, state: "available", detection: { status: "found", path: "/usr/local/bin/gopls", hint: null } }),
  ],
};

/** Stub `ext_*` (and record `settings_set`) on the shared bridge, record `on`
 *  handlers, re-seed the store. */
const seedRegistry = (view: unknown) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  const store = bar.extensions;
  const bridge = store["bridge"];
  const origInvoke = bridge.invoke.bind(bridge);
  const ext = (window.__ext = { calls: [], handlers: {} });
  bridge.invoke = (cmd, args) => {
    if (cmd.startsWith("ext_") || cmd === "settings_set") {
      ext.calls.push([cmd, args]);
      return Promise.resolve(cmd === "ext_registry_list" ? ${JSON.stringify(view)} : null);
    }
    return origInvoke(cmd, args);
  };
  bridge.on = (event, handler) => {
    (ext.handlers[event] ||= []).push(handler);
    return Promise.resolve(() => {});
  };
  store.connect();
})()`;

const emit = (event: string, payload: unknown) =>
  `(window.__ext.handlers[${JSON.stringify(event)}] || []).forEach((h) => h(${JSON.stringify(payload)}))`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedRegistry(VIEW));
  await expect(page.locator(".tb-extensions")).toBeVisible();
  // the seed landed → the dot shows (an update + a pending restart)
  await expect(page.locator(".tb-extensions .ext-dot")).toBeVisible();
}

const dialog = (page: Page) => page.locator("app-extensions-modal[role=dialog]");
const row = (page: Page, id: string) => dialog(page).locator(`.ext-row[data-ext-id="${id}"]`);
const systemToggle = (page: Page) => dialog(page).locator('.kj-toggle[aria-label="Use system servers"]');
/** Rendered text with the CSS-gap dots normalised to " · ". */
const flat = (l: ReturnType<Page["locator"]>) => l.evaluate((el) => (el.textContent ?? "").replace(/\s+/g, " ").replace(/\s*·\s*/g, " · ").trim());

test("puzzle button sits immediately left of the settings gear and opens the panel", async ({ page }) => {
  await boot(page);
  // DOM order inside the action pill: … · divider · extensions · divider · settings
  const order = await page.evaluate(() => {
    const ext = document.querySelector(".tb-extensions")!;
    const a = ext.nextElementSibling!;
    const b = a.nextElementSibling!;
    return [ext.previousElementSibling?.classList.contains("pill-div"), a.classList.contains("pill-div"), b.classList.contains("tb-settings"), b.nextElementSibling === null];
  });
  expect(order).toEqual([true, true, true, true]);
  await expect(page.locator(".tb-extensions")).toHaveAttribute("title", /Extensions · Ctrl\+Shift\+X/);

  await page.locator(".tb-extensions").click();
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).locator(".set-head .ht")).toHaveText("Grammars");
  const nav = dialog(page).locator(".set-nav-item");
  await expect(nav).toHaveText(["Grammars", "Language servers", "Updates1"]);
  await expect(dialog(page).locator(".ext-count")).toHaveText("1");
});

test("Ctrl+Shift+X opens the panel; Escape closes it", async ({ page }) => {
  await boot(page);
  await page.keyboard.press("Control+Shift+X");
  await expect(dialog(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveCount(0);
  // and the store flag followed the overlay
  expect(await page.evaluate(`window.ng.getComponent(document.querySelector("app-top-bar")).extensions.open()`)).toBe(false);
});

test("rows render per state", async ({ page }) => {
  await boot(page);
  await page.locator(".tb-extensions").click();
  await expect(row(page, "rust")).toHaveAttribute("data-state", "update");
  await expect(row(page, "rust").locator(".ext-up")).toHaveText("0.23.4 → 0.24.1");
  await expect(row(page, "rust").getByRole("button", { name: "Update" })).toBeVisible();
  await expect(row(page, "ts")).toHaveAttribute("data-state", "installed");
  await expect(row(page, "ts").locator(".kj-toggle")).toHaveAttribute("aria-pressed", "true");
  await expect(row(page, "ts").getByRole("button", { name: "Uninstall" })).toBeVisible();
  await expect(row(page, "java")).toHaveClass(/dim/);
  await expect(row(page, "java").locator(".kj-toggle")).toHaveAttribute("aria-pressed", "false");
  await expect(row(page, "cpp")).toHaveAttribute("data-state", "downloading");
  await expect(row(page, "cpp").locator(".ext-bar .kj-progress-bar")).toBeVisible();
  await expect(row(page, "kotlin").getByRole("button", { name: "Install" })).toBeVisible();
  await expect(row(page, "go").locator(".ext-badge")).toHaveText("restart to activate");
  await expect(row(page, "go").getByRole("button", { name: "Restart now" })).toBeVisible();
  await expect(row(page, "zig")).toHaveClass(/err/);
  await expect(row(page, "zig").locator(".ext-line.err")).toContainText("sha256 mismatch");
  await expect(row(page, "zig").getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(row(page, "hs")).toHaveClass(/dim/);
  await expect(row(page, "hs")).toContainText("needs Orrery ≥ 0.24");
  // server packs live in their own section with the detection + hint lines
  await expect(row(page, "jdtls")).toHaveCount(0);
  await dialog(page).locator(".set-nav-item", { hasText: "Language servers" }).click();
  // a manually configured path is an explicit choice: shown (and honoured by
  // the backend) whether or not "Use system servers" is on
  await expect(row(page, "jdtls").locator(".ext-line.mute")).toHaveCount(0);
  await expect(row(page, "jdtls").locator(".ext-line .lb")).toHaveText("configured ·");
  await systemToggle(page).click();
  await expect(row(page, "jdtls").locator(".ext-line .lb")).toHaveText("configured ·");
  await expect(row(page, "jdtls").locator(".ext-line .mono")).toHaveText("/opt/jdtls/bin/jdtls");
  await expect(row(page, "jdtls").locator(".ext-line.warn")).toContainText("needs JDK 17+");
  await expect(row(page, "jdtls").getByRole("button", { name: "Locate…" })).toBeVisible();
  await expect(row(page, "clangd").getByRole("button", { name: "Install" })).toBeVisible();
  await expect(dialog(page).locator(".set-num-idle input")).toHaveValue("10");
  // the uninstall confirm pops with the design copy
  await dialog(page).locator(".set-nav-item", { hasText: "Grammars" }).click();
  await row(page, "ts").getByRole("button", { name: "Uninstall" }).click();
  // kouji keeps one (hidden) panel per trigger in the DOM — pick the open one
  const confirm = page.locator(".ext-confirm:not([hidden])");
  await expect(confirm).toBeVisible();
  await expect(confirm).toContainText("Uninstall TypeScript & TSX?");
  await expect(confirm).toContainText("fall back to plain text");
  await confirm.getByRole("button", { name: "Keep" }).click();
  await expect(page.locator('.ext-confirm[data-state="open"], .ext-confirm[data-state="opening"]')).toHaveCount(0);
  // kouji-ui ≥ core 0.8.2: the popup closes ITSELF, not the enclosing dialog
  await expect(page.locator(".ext-modal")).toBeVisible();
  await expect(row(page, "ts").getByRole("button", { name: "Uninstall" })).toBeVisible();
});

test("Install invokes ext_install; a progress tick fills the bar", async ({ page }) => {
  await boot(page);
  await page.locator(".tb-extensions").click();
  await row(page, "kotlin").getByRole("button", { name: "Install" }).click();
  await expect
    .poll(() => page.evaluate(`window.__ext.calls.filter(([c]) => c === "ext_install").map(([, a]) => a)`))
    .toEqual([{ id: "kotlin" }]);

  // the backend would answer with a status flipping kotlin to downloading…
  const downloading = {
    ...VIEW,
    items: VIEW.items.map((p) => (p.id === "kotlin" ? { ...p, state: "downloading" } : p)),
  };
  await page.evaluate(emit("ext://status", downloading));
  await expect(row(page, "kotlin")).toHaveAttribute("data-state", "downloading");
  await expect(row(page, "kotlin").locator(".ext-fig")).toHaveText("starting…");
  // …then progress ticks
  await page.evaluate(emit("ext://progress", { id: "kotlin", downloaded: 12.3 * MB, total: 40 * MB, phase: "download" }));
  await expect(row(page, "kotlin").locator(".ext-fig")).toHaveText("12.3 / 40 MB");
  await expect(row(page, "kotlin").locator("[role=progressbar]")).toHaveAttribute("aria-valuenow", "31");
  // the fill is real: it grew past zero width
  const width = await row(page, "kotlin").locator(".kj-progress-bar__fill").evaluate((el) => el.getBoundingClientRect().width);
  expect(width).toBeGreaterThan(0);
  // a status that says installed drops the bar and shows the toggle
  const installed = {
    ...VIEW,
    items: VIEW.items.map((p) => (p.id === "kotlin" ? { ...p, state: "installed", installed: true, enabled: true, installedVersion: "1.0.0" } : p)),
  };
  await page.evaluate(emit("ext://status", installed));
  await expect(row(page, "kotlin")).toHaveAttribute("data-state", "installed");
  await expect(row(page, "kotlin").locator(".ext-bar")).toHaveCount(0);
  await expect(row(page, "kotlin").locator(".kj-toggle")).toHaveAttribute("aria-pressed", "true");
});

test("self-contained server packs: bundle line, priced Install, bundled version, runtime lock", async ({ page }) => {
  await boot(page);
  await page.locator(".tb-extensions").click();
  await dialog(page).locator(".set-nav-item", { hasText: "Language servers" }).click();
  // before install: what the bundle pulls in, and the Install button prices it
  const ts = row(page, "tsserver");
  expect(await flat(ts.locator(".ext-bundle"))).toBe("bundled · includes Node runtime · 58 MB");
  await expect(ts.getByRole("button", { name: "Install · 58 MB" })).toBeVisible();
  expect(await flat(row(page, "clangd").locator(".ext-bundle"))).toBe("bundled · 38 MB");
  // after install: the bundled binary's version, path on hover
  const py = row(page, "pyright");
  expect(await flat(py.locator(".ext-line").first())).toBe("bundled · v1.1.400");
  await expect(py.locator(".ext-line .mono")).toHaveAttribute("title", "C:/packs/pyright/langserver.js");
  await expect(py.getByRole("button", { name: "Locate…" })).toHaveCount(0);
  await expect(py.locator(".kj-toggle")).toHaveAttribute("aria-pressed", "true");
  // the Runtimes group: auto-managed, no Enabled toggle, Uninstall locked with the reason
  const grp = dialog(page).locator('[data-testid="ext-runtimes"]');
  await expect(grp.locator(".set-grp-h")).toHaveText("Runtimes");
  const node = grp.locator('.ext-row[data-ext-id="runtime.node"]');
  await expect(node.locator(".set-vchip")).toHaveText("v22.4.0");
  expect(await flat(node.locator(".ext-meta"))).toBe("auto-managed · used by Pyright · 46 MB");
  await expect(node.locator(".kj-toggle")).toHaveCount(0);
  await expect(node.locator(".ext-lock")).toHaveAttribute("title", "required by Pyright");
  await expect(node.locator(".ext-lock .kj-button")).toHaveAttribute("aria-disabled", "true");
  await expect(dialog(page).locator(".set-nav-foot")).toContainText("7 packs");
  // grammar rows keep the plain label
  await dialog(page).locator(".set-nav-item", { hasText: "Grammars" }).click();
  await expect(row(page, "kotlin").getByRole("button", { name: "Install", exact: true })).toBeVisible();
});

test("a server install streams its runtime first: the bar names the step", async ({ page }) => {
  await boot(page);
  await page.locator(".tb-extensions").click();
  await dialog(page).locator(".set-nav-item", { hasText: "Language servers" }).click();
  await row(page, "tsserver").getByRole("button", { name: "Install · 58 MB" }).click();
  await expect
    .poll(() => page.evaluate(`window.__ext.calls.filter(([c]) => c === "ext_install").map(([, a]) => a)`))
    .toEqual([{ id: "tsserver" }]);
  const downloading = { ...VIEW, items: VIEW.items.map((p) => (p.id === "tsserver" ? { ...p, state: "downloading" } : p)) };
  await page.evaluate(emit("ext://status", downloading));
  const ts = row(page, "tsserver");
  await expect(ts).toHaveAttribute("data-state", "downloading");
  await expect(ts.locator(".ext-bundle")).toHaveCount(0);
  await expect(ts.locator(".ext-fig")).toHaveText("starting…");
  // step 1: the dependency, under the REQUESTED pack's id
  await page.evaluate(emit("ext://progress", { id: "tsserver", dependency: "runtime.node", stepIndex: 1, stepCount: 2, downloaded: 12.3 * MB, total: 32 * MB, phase: "download" }));
  await expect(ts.locator(".ext-fig")).toHaveText("1/2 · Node runtime 22 · 12.3 / 32 MB");
  await expect(ts.locator("[role=progressbar]")).toHaveAttribute("aria-valuenow", "38");
  // step 2: the server itself
  await page.evaluate(emit("ext://progress", { id: "tsserver", dependency: null, stepIndex: 2, stepCount: 2, downloaded: 3.1 * MB, total: 12 * MB, phase: "download" }));
  await expect(ts.locator(".ext-fig")).toHaveText("2/2 · TypeScript language server · 3.1 / 12 MB");
  // landed: bundled line with the version, Enabled toggle, no bar
  const installed = {
    ...VIEW,
    items: VIEW.items.map((p) =>
      p.id === "tsserver" ? { ...p, state: "installed", installed: true, enabled: true, version: "6.0.0", installedVersion: "6.0.0", detection: { status: "bundled", path: "C:/packs/tsserver/bin/server.js", hint: null } } : p,
    ),
  };
  await page.evaluate(emit("ext://status", installed));
  await expect(ts).toHaveAttribute("data-state", "installed");
  await expect(ts.locator(".ext-bar")).toHaveCount(0);
  expect(await flat(ts.locator(".ext-line").first())).toBe("bundled · v6.0.0");
  await expect(ts.locator(".kj-toggle")).toHaveAttribute("aria-pressed", "true");
});

test("'Use system servers' persists lspUseSystemServers and reveals a found-on-PATH server", async ({ page }) => {
  await boot(page);
  await page.locator(".tb-extensions").click();
  await dialog(page).locator(".set-nav-item", { hasText: "Language servers" }).click();
  await expect(dialog(page).locator(".set-row", { hasText: "Use system servers" })).toContainText("Fall back to servers found on PATH");
  await expect(systemToggle(page)).toHaveAttribute("aria-pressed", "false");
  const go = row(page, "gopls");
  await expect(go.locator(".ext-line.mute")).toHaveText("system copy ignored — enable ‘Use system servers’");
  await expect(go.locator(".ext-line.mute")).toHaveAttribute("title", "/usr/local/bin/gopls");
  await expect(go.getByRole("button", { name: "Locate…" })).toHaveCount(0);
  await systemToggle(page).click();
  await expect(systemToggle(page)).toHaveAttribute("aria-pressed", "true");
  // the debounced settings_set carries the flag
  await expect
    .poll(() => page.evaluate(`window.__ext.calls.filter(([c]) => c === "settings_set").map(([, a]) => a.settings.lspUseSystemServers)`))
    .toContain(true);
  await expect(go.locator(".ext-line.mute")).toHaveCount(0);
  await expect(go.locator(".ext-line .lb").last()).toHaveText("found ·");
  await expect(go.locator(".ext-line .mono:not(.tnum)")).toHaveText("/usr/local/bin/gopls");
  await expect(go.getByRole("button", { name: "Locate…" })).toBeVisible();
  // still an Install away from a self-contained copy
  await expect(go.getByRole("button", { name: "Install · 20 MB" })).toBeVisible();
});
