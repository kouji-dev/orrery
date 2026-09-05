import { test, expect, type Page } from "@playwright/test";
import { createServer, type Server } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join } from "node:path";

/**
 * Landing direct-download routing (landing/version.js).
 *
 * The landing is a static folder outside the Angular app, so this spec serves
 * it with its own tiny http server instead of the config's webServer, and mocks
 * the GitHub releases API at the network layer:
 *   - success → every .js-dl CTA is rewritten to the direct installer asset for
 *     the visitor's OS (win → -setup.exe, mac → .dmg) and target=_blank dropped;
 *   - API failure → the baked /releases/latest page fallback stays untouched.
 * navigator.platform is overridden per test so the OS branch under test never
 * depends on the machine running the suite.
 */

const LANDING = join(process.cwd(), "landing");
const API = "https://api.github.com/repos/kouji-dev/orrery-releases/releases/latest";
const DL = "https://github.com/kouji-dev/orrery-releases/releases/download/v9.9.9";
const EXE = `${DL}/Orrery_9.9.9_x64-setup.exe`;
const MSI = `${DL}/Orrery_9.9.9_x64_en-US.msi`;
const DMG = `${DL}/Orrery_9.9.9_aarch64.dmg`;
const RELEASE = {
  tag_name: "v9.9.9",
  assets: [
    { name: "Orrery_9.9.9_x64-setup.exe", browser_download_url: EXE },
    { name: "Orrery_9.9.9_x64_en-US.msi", browser_download_url: MSI },
    { name: "Orrery_9.9.9_aarch64.dmg", browser_download_url: DMG },
    { name: "latest.json", browser_download_url: `${DL}/latest.json` },
  ],
};

const MIME: Record<string, string> = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
};

let server: Server;
let base: string;

test.beforeAll(async () => {
  server = createServer(async (req, res) => {
    const path = (req.url ?? "/").split("?")[0];
    const file = path === "/" ? "/index.html" : path;
    try {
      const body = await readFile(join(LANDING, file));
      res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
      res.end(body);
    } catch {
      res.writeHead(404).end("not found");
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address();
  base = `http://127.0.0.1:${typeof addr === "object" && addr ? addr.port : 0}/`;
});

test.afterAll(async () => {
  await new Promise((resolve) => server.close(resolve));
});

/** Fresh page: cleared cache, forced platform, third-party requests stubbed. */
async function open(page: Page, opts: { platform: string; apiStatus?: number }) {
  await page.addInitScript((platform) => {
    localStorage.clear();
    Object.defineProperty(navigator, "platform", { get: () => platform });
  }, opts.platform);
  // Keep the test hermetic: no gtag/fonts traffic off-box.
  await page.route(/googletagmanager\.com|fonts\.(googleapis|gstatic)\.com/, (r) => r.fulfill({ status: 204, body: "" }));
  await page.route(API, (r) =>
    opts.apiStatus
      ? r.fulfill({ status: opts.apiStatus, contentType: "application/json", body: "{}" })
      : r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(RELEASE) }),
  );
  // All-releases list — feeds the total download counter.
  await page.route(/\/releases\?per_page/, (r) =>
    opts.apiStatus
      ? r.fulfill({ status: opts.apiStatus, contentType: "application/json", body: "[]" })
      : r.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify([
            { tag_name: "v9.9.9", assets: [
              { name: "Orrery_9.9.9_x64-setup.exe", download_count: 1000 },
              { name: "Orrery_9.9.9_aarch64.dmg", download_count: 200 },
              { name: "latest.json", download_count: 99999 }, // update checks — must NOT be counted
            ]},
            { tag_name: "v9.9.8", assets: [{ name: "Orrery_9.9.8_x64_en-US.msi", download_count: 34 }] },
          ]),
        }),
  );
  await page.goto(base);
}

test("windows visitor: CTAs become direct -setup.exe downloads, dropdown holds the rest", async ({ page }) => {
  await open(page, { platform: "Win32" });

  const ctas = page.locator(".js-dl");
  await expect(ctas).toHaveCount(3);
  for (const cta of await ctas.all()) {
    await expect(cta).toHaveAttribute("href", EXE);
    await expect(cta).not.toHaveAttribute("target", "_blank");
  }
  await expect(page.locator(".js-ver").first()).toHaveText("v9.9.9");
  await expect(page.locator(".js-dl-label").first()).toHaveText("Windows");
  await expect(page.locator(".js-dl-count")).toHaveText(/1,234\sdownloads/); // exe+dmg+msi, update checks excluded

  // Arrow opens the dropdown with the other installers.
  const split = page.locator(".dl-split").first();
  await split.locator(".js-dl-arrow").click();
  await expect(split).toHaveClass(/open/);
  const menu = split.locator(".js-dl-menu");
  await expect(menu.getByText(".msi installer")).toHaveAttribute("href", MSI);
  // macOS is "coming soon": a disabled row, never a link — even though the
  // release DOES carry a .dmg asset
  const mac = menu.locator(".dl-item.soon");
  await expect(mac).toContainText("macOS · Apple Silicon");
  await expect(mac).toContainText("coming soon");
  await expect(mac).toHaveAttribute("aria-disabled", "true");
  await expect(mac).not.toHaveAttribute("href", /.*/);
  await expect(menu.locator(`a[href="${DMG}"]`)).toHaveCount(0);
  await expect(menu.getByText("All releases on GitHub")).toHaveAttribute("href", /releases$/);

  // Items must actually be hittable (regression: a revealed section once painted
  // over the menu and swallowed clicks) — a real click fires the download request.
  await page.route("**/releases/download/**", (r) =>
    r.fulfill({ status: 200, contentType: "application/octet-stream", body: "x" }),
  );
  const [req] = await Promise.all([page.waitForRequest(MSI), menu.getByText(".msi installer").click()]);
  expect(req.url()).toBe(MSI);

  // Outside click closes it.
  await split.locator(".js-dl-arrow").click();
  await page.locator(".hero-h1").click();
  await expect(split).not.toHaveClass(/open/);
});

test("mac visitor: no mac build yet — the Windows installer stays the CTA, macOS reads coming soon", async ({ page }) => {
  await open(page, { platform: "MacIntel" });

  for (const cta of await page.locator(".js-dl").all()) {
    await expect(cta).toHaveAttribute("href", EXE);
    await expect(cta).not.toHaveAttribute("target", "_blank");
  }
  await expect(page.locator(".js-dl-label").first()).toHaveText("Windows");

  const split = page.locator(".dl-split").first();
  await split.locator(".js-dl-arrow").click();
  const menu = split.locator(".js-dl-menu");
  await expect(menu.locator(".dl-item.soon")).toContainText("coming soon");
  await expect(menu.locator(`a[href="${DMG}"]`)).toHaveCount(0);
  await expect(menu).not.toContainText("xattr"); // the Gatekeeper hint went with the .dmg
});

test("agent chips render via <orrery-agent> with brand icons", async ({ page }) => {
  await open(page, { platform: "Win32" });

  const strip = page.locator(".toolstrip");
  await expect(strip.locator("orrery-agent")).toHaveCount(5); // 4 tools + "and more"
  await expect(strip.locator("orrery-agent svg use")).toHaveCount(5);
  await expect(strip).toContainText("and more");
  await expect(page.locator(".spin orrery-agent")).toHaveCount(4); // hero orbit nodes
  await expect(page.locator(".spin")).toContainText("Claude Code");
});

test("API failure: baked releases-page fallback stays untouched", async ({ page }) => {
  await open(page, { platform: "Win32", apiStatus: 403 });

  const ctas = page.locator(".js-dl");
  await expect(ctas).toHaveCount(3);
  for (const cta of await ctas.all()) {
    await expect(cta).toHaveAttribute("href", /\/releases\/latest$/);
    await expect(cta).toHaveAttribute("target", "_blank");
  }
  await expect(page.locator(".js-dl-count")).toBeHidden();
  // The baked dropdown still opens and offers the releases page (plus the
  // static macOS "coming soon" row, which needs no release data).
  const split = page.locator(".dl-split").first();
  await split.locator(".js-dl-arrow").click();
  await expect(split).toHaveClass(/open/);
  const links = split.locator("a.dl-item");
  await expect(links).toHaveCount(1);
  await expect(links.first()).toHaveAttribute("href", /releases$/);
  await expect(split.locator(".dl-item.soon")).toContainText("coming soon");
});
