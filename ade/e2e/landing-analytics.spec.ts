import { test, expect, type Page } from "@playwright/test";
import { createServer, type Server } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join } from "node:path";

/**
 * Landing GA4 events (landing/analytics.js + version.js release_lookup).
 *
 * Same hermetic harness as landing-download.spec.ts: the landing folder is
 * served by a tiny http server and the GitHub API + gtag.js are stubbed at the
 * network layer. The inline gtag bootstrap still defines window.gtag and
 * pushes into window.dataLayer, so events are asserted straight off dataLayer
 * without any Google traffic leaving the box.
 */

const LANDING = join(process.cwd(), "landing");
const API = "https://api.github.com/repos/kouji-dev/orrery-releases/releases/latest";
const DL = "https://github.com/kouji-dev/orrery-releases/releases/download/v9.9.9";
const EXE = `${DL}/Orrery_9.9.9_x64-setup.exe`;
const RELEASE = {
  tag_name: "v9.9.9",
  assets: [
    { name: "Orrery_9.9.9_x64-setup.exe", browser_download_url: EXE },
    { name: "Orrery_9.9.9_aarch64.dmg", browser_download_url: `${DL}/Orrery_9.9.9_aarch64.dmg` },
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

async function open(page: Page, opts: { apiStatus?: number } = {}) {
  await page.addInitScript(() => {
    localStorage.clear();
    Object.defineProperty(navigator, "platform", { get: () => "Win32" });
  });
  await page.route(/googletagmanager\.com|fonts\.(googleapis|gstatic)\.com/, (r) => r.fulfill({ status: 204, body: "" }));
  await page.route(API, (r) =>
    opts.apiStatus
      ? r.fulfill({ status: opts.apiStatus, contentType: "application/json", body: "{}" })
      : r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(RELEASE) }),
  );
  await page.route(/\/releases\?per_page/, (r) => r.fulfill({ status: 200, contentType: "application/json", body: "[]" }));
  await page.goto(base);
}

/** [name, params] pairs of every gtag('event', …) pushed so far. */
const events = (page: Page) =>
  page.evaluate(() =>
    ((window as never as { dataLayer: IArguments[] }).dataLayer || [])
      .filter((a) => a[0] === "event")
      .map((a) => [a[1], a[2] ?? {}] as [string, Record<string, unknown>]),
  );

test("installer CTA click lands file_download; release lookup reports ok", async ({ page }) => {
  await open(page);
  await expect(page.locator(".js-dl").first()).toHaveAttribute("href", EXE);

  await page.route("**/releases/download/**", (r) =>
    r.fulfill({ status: 200, contentType: "application/octet-stream", body: "x" }),
  );
  await page.locator(".js-dl").first().click();

  const evs = await events(page);
  const names = evs.map(([n]) => n);
  expect(names).toContain("release_lookup");
  expect(evs.find(([n]) => n === "release_lookup")?.[1]).toMatchObject({ status: "ok", version: "v9.9.9" });
  const dl = evs.find(([n]) => n === "file_download");
  expect(dl?.[1]).toMatchObject({ file_name: "Orrery_9.9.9_x64-setup.exe", platform: "win" });
});

test("outbound GitHub link is tracked as an outbound click", async ({ page }) => {
  await open(page);
  // capture synchronously after dispatch, before any navigation can commit
  const evs = await page.evaluate(() => {
    const a = document.querySelector<HTMLAnchorElement>('a[href="https://github.com/kouji-dev/orrery"]')!;
    a.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    return ((window as never as { dataLayer: IArguments[] }).dataLayer || [])
      .filter((x) => x[0] === "event")
      .map((x) => [x[1], x[2] ?? {}] as [string, Record<string, unknown>]);
  });
  const out = evs.find(([n]) => n === "click");
  expect(out?.[1]).toMatchObject({ outbound: true, link_domain: "github.com" });
});

test("API failure reports release_lookup error and no file_download on the fallback CTA", async ({ page }) => {
  await open(page, { apiStatus: 403 });
  await expect(page.locator(".js-dl").first()).toHaveAttribute("href", /\/releases\/latest$/);

  const evs = await page.evaluate(() => {
    const a = document.querySelector<HTMLAnchorElement>(".js-dl")!;
    a.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    return ((window as never as { dataLayer: IArguments[] }).dataLayer || [])
      .filter((x) => x[0] === "event")
      .map((x) => [x[1], x[2] ?? {}] as [string, Record<string, unknown>]);
  });
  const names = evs.map(([n]) => n);
  expect(evs.find(([n]) => n === "release_lookup")?.[1]).toMatchObject({ status: "error" });
  expect(names).not.toContain("file_download");
  expect(evs.find(([n]) => n === "click")?.[1]).toMatchObject({ outbound: true }); // fallback goes to github.com
});
