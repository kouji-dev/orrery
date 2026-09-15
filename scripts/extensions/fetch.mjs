import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { sha256 } from './build-grammar.mjs';

// Download-once cache for upstream assets (`.ext-build/downloads/<file>`),
// verified against the pinned sha256 on every use — a corrupt cache entry is
// deleted so the next run re-downloads instead of failing forever.

/** Cache file name for a URL: its last path segment, percent-decoded. */
export function cacheName(url) {
  return decodeURIComponent(basename(new URL(url).pathname));
}

/** Verify `buf` against the pin. Throws on mismatch; warns when unpinned. */
export function verify(buf, expected, what) {
  const digest = sha256(buf);
  if (expected && expected !== digest) {
    throw new Error(`sha256 mismatch for ${what}: pinned ${expected}, got ${digest}`);
  }
  if (!expected) console.warn(`${what}: no sha256 pinned — set "sha256": "${digest}"`);
  return digest;
}

export async function download(url) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`${url} → HTTP ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

/** `{ url, sha256 }` → verified Buffer; cached under `cacheDir`. */
export async function fetchAsset({ url, sha256: expected }, cacheDir) {
  const file = join(cacheDir, cacheName(url));
  if (existsSync(file)) {
    const buf = readFileSync(file);
    try {
      verify(buf, expected, file);
      console.log(`using cached ${file}`);
      return buf;
    } catch (e) {
      console.warn(`${e.message} — re-downloading`);
      rmSync(file);
    }
  }
  console.log(`downloading ${url}`);
  const buf = await download(url);
  verify(buf, expected, url);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, buf);
  return buf;
}
