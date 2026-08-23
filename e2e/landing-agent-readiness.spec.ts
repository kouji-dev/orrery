import { test, expect, type Page } from "@playwright/test";
import { createServer, type Server } from "node:http";
import { readFile, readdir } from "node:fs/promises";
import { extname, join } from "node:path";

/**
 * Agent readiness of the landing site — the machine-readable surface an AI
 * crawler sees: /llms.txt, /robots.txt, /sitemap.xml, /og.png, the homepage's
 * JSON-LD + canonical + Open Graph signals, and its heading hierarchy.
 *
 * Same hermetic harness as landing-analytics.spec.ts: the landing folder is
 * served by a tiny http server (MIME map extended to the new file types) and
 * every outbound request — GitHub API, gtag.js, Google Fonts — is stubbed, so
 * nothing leaves the box.
 *
 * The sitemap assertion is deliberately filesystem-driven: add a page to
 * landing/ without running scripts/landing/gen-sitemap.mjs and this fails.
 */

const LANDING = join(process.cwd(), "landing");
const ORIGIN = "https://orrery.kouji.dev";

const MIME: Record<string, string> = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
  ".xml": "application/xml",
  ".png": "image/png",
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

/** Open a landing page with every third-party call stubbed out. */
async function open(page: Page, path = "") {
  await page.route(/googletagmanager\.com|fonts\.(googleapis|gstatic)\.com/, (r) =>
    r.fulfill({ status: 204, body: "" }),
  );
  await page.route(/api\.github\.com/, (r) =>
    r.fulfill({ status: 200, contentType: "application/json", body: "{}" }),
  );
  await page.goto(base + path);
}

const text = (file: string) => readFile(join(LANDING, file), "utf8");

test.describe("machine-readable files", () => {
  test("llms.txt follows the llmstxt.org shape and names when to use Orrery", async ({ request }) => {
    const res = await request.get(base + "llms.txt");
    expect(res.status()).toBe(200);
    expect(res.headers()["content-type"]).toContain("text/plain");

    const body = await res.text();
    const lines = body.split(/\r?\n/);

    // spec order: single H1, then a blockquote summary, then H2 sections
    expect(lines[0]).toBe("# Orrery");
    expect(lines.filter((l) => /^# /.test(l))).toHaveLength(1);
    expect(body).toMatch(/\n> /);

    // the finding this file exists to fix: explicit when-to-use guidance
    const sections = lines.filter((l) => l.startsWith("## ")).map((l) => l.slice(3));
    expect(sections).toContain("When to use Orrery");
    const whenToUse = body.split("## When to use Orrery")[1].split("\n## ")[0];
    expect(whenToUse.split(/\n- /).length).toBeGreaterThanOrEqual(4);

    // link lists must be real markdown links pointing at real hosts
    for (const url of body.match(/\]\((https?:[^)]+)\)/g) ?? []) {
      expect(url).toMatch(/orrery\.kouji\.dev|github(usercontent)?\.com|kouji\.dev/);
    }
  });

  test("robots.txt allows crawling and points at the sitemap", async ({ request }) => {
    const res = await request.get(base + "robots.txt");
    expect(res.status()).toBe(200);

    const body = await res.text();
    expect(body).toMatch(/^User-agent: \*$/m);
    expect(body).toMatch(/^Allow: \/$/m);
    expect(body).toMatch(new RegExp(`^Sitemap: ${ORIGIN}/sitemap\\.xml$`, "m"));
    expect(body).not.toMatch(/^Disallow: \/$/m);

    // AI crawlers are permitted explicitly, not just by the wildcard group
    for (const ua of ["GPTBot", "ClaudeBot", "PerplexityBot", "Google-Extended"]) {
      expect(body).toContain(`User-agent: ${ua}`);
    }
  });

  test("sitemap.xml is valid and lists every page in landing/", async ({ request }) => {
    const res = await request.get(base + "sitemap.xml");
    expect(res.status()).toBe(200);
    expect(res.headers()["content-type"]).toContain("xml");

    const body = await res.text();
    expect(body.startsWith('<?xml version="1.0" encoding="UTF-8"?>')).toBe(true);
    expect(body).toContain('<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">');

    const locs = [...body.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => m[1]);
    const mods = [...body.matchAll(/<lastmod>([^<]+)<\/lastmod>/g)].map((m) => m[1]);
    expect(locs.length).toBe(mods.length);
    for (const loc of locs) expect(loc.startsWith(`${ORIGIN}/`)).toBe(true);
    for (const mod of mods) {
      expect(mod).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(Number.isNaN(Date.parse(mod))).toBe(false);
    }

    // every shipped page is listed; the error page never is
    const pages = (await readdir(LANDING)).filter((f) => f.endsWith(".html"));
    const expected = pages
      .filter((f) => f !== "404.html")
      .map((f) => (f === "index.html" ? `${ORIGIN}/` : `${ORIGIN}/${f}`));
    expect(locs.sort()).toEqual(expected.sort());
    expect(locs).not.toContain(`${ORIGIN}/404.html`);
  });

  test("og.png is a real 1200x630 PNG", async ({ request }) => {
    const res = await request.get(base + "og.png");
    expect(res.status()).toBe(200);
    expect(res.headers()["content-type"]).toBe("image/png");

    const png = await res.body();
    expect([...png.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    // IHDR: width and height are big-endian uint32 at byte 16 and 20
    expect(png.readUInt32BE(16)).toBe(1200);
    expect(png.readUInt32BE(20)).toBe(630);
  });
});

test.describe("homepage markup", () => {
  test("carries all four metadata signals", async ({ page }) => {
    await open(page);

    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute("href", `${ORIGIN}/`);
    await expect(page.locator('meta[property="og:type"]')).toHaveAttribute("content", "website");
    await expect(page.locator('meta[property="og:image"]')).toHaveAttribute(
      "content",
      `${ORIGIN}/og.png`,
    );
    // the card needs its dimensions to render without a fetch
    await expect(page.locator('meta[property="og:image:width"]')).toHaveAttribute("content", "1200");
    await expect(page.locator('meta[property="og:image:height"]')).toHaveAttribute("content", "630");
  });

  test("exposes SoftwareApplication JSON-LD", async ({ page }) => {
    await open(page);

    const blocks = await page.locator('script[type="application/ld+json"]').allTextContents();
    expect(blocks).toHaveLength(1);

    const ld = JSON.parse(blocks[0]);
    expect(ld["@context"]).toBe("https://schema.org");
    expect(ld["@type"]).toBe("SoftwareApplication");
    expect(ld.name).toBe("Orrery");
    expect(ld.url).toBe(`${ORIGIN}/`);
    expect(ld.description.length).toBeGreaterThan(60);
    expect(ld.applicationCategory).toBe("DeveloperApplication");
    expect(ld.operatingSystem).toContain("Windows");
    expect(ld.offers.price).toBe("0");
    expect(ld.sameAs).toContain("https://github.com/kouji-dev/orrery");
    expect(ld.publisher.name).toBe("Kouji.dev");
    // deliberately absent: version.js owns the live version, a baked one goes stale
    expect(ld.softwareVersion).toBeUndefined();
  });

  test("has a nested heading hierarchy with a single h1", async ({ page }) => {
    await open(page);

    const levels = await page.$$eval("h1,h2,h3,h4,h5,h6", (els) =>
      els.map((e) => Number(e.tagName[1])),
    );
    expect(levels.filter((l) => l === 1)).toHaveLength(1);
    expect(levels[0]).toBe(1);
    expect(levels).toEqual([1, 2, 2, 3, 3, 3, 2]);
    // no skipped levels — h1 → h3 with no h2 between reads as flat to a parser
    for (let i = 1; i < levels.length; i++) {
      expect(levels[i] - levels[i - 1]).toBeLessThanOrEqual(1);
    }

    // the promoted nodes are still the same copy, in the same place
    await expect(page.locator("h2.kicker")).toHaveText("How it works");
    await expect(page.locator("h3.feat-t")).toHaveText([
      "Dispatch a fleet",
      "A worktree per agent",
      "Watch every branch live",
    ]);
  });

  test("renders meaningful text without JavaScript", async ({ browser }) => {
    // raw HTML only: what a crawler that never executes a script would see
    const ctx = await browser.newContext({ javaScriptEnabled: false });
    const page = await ctx.newPage();
    await page.route(/googletagmanager\.com|fonts\.(googleapis|gstatic)\.com/, (r) =>
      r.fulfill({ status: 204, body: "" }),
    );
    await page.goto(base);

    await expect(page.locator("h1")).toHaveCount(1);
    const body = (await page.locator("main").innerText()).replace(/\s+/g, " ").trim();
    expect(body.length).toBeGreaterThan(500);
    await ctx.close();
  });

  test("changelog.html is canonical and shares the card", async ({ page }) => {
    await open(page, "changelog.html");
    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute(
      "href",
      `${ORIGIN}/changelog.html`,
    );
    await expect(page.locator('meta[property="og:image"]')).toHaveAttribute(
      "content",
      `${ORIGIN}/og.png`,
    );
  });
});

test("404 page points agents at the recovery paths", async ({ page }) => {
  await open(page, "404.html");

  // human-facing links
  for (const href of ["/", "/changelog.html", "/llms.txt", "/sitemap.xml", "/robots.txt"]) {
    await expect(page.locator(`a[href="${href}"]`)).toHaveCount(1);
  }
  // and the same routes as a markdown block, for agents reading the body
  const md = await page.locator("pre").innerText();
  expect(md).toContain(`[llms.txt](${ORIGIN}/llms.txt)`);
  expect(md).toContain(`[sitemap.xml](${ORIGIN}/sitemap.xml)`);

  // never indexable, and never listed in the sitemap
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute("content", "noindex");
  expect(await text("sitemap.xml")).not.toContain("404.html");
});
