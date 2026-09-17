import { mkdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { MIN_ORRERY_VERSION, TARGETS, packName, parseArgs } from './build-grammar.mjs';
import { loadServerPins } from './build-servers.mjs';
import { fetchAsset } from './fetch.mjs';
import { dereference, readTarGz, stripComponents } from './tar.mjs';
import { readZipEntries, writeZip } from './zip.mjs';

// Runtime packs: the interpreters the node/java server packs need, so an
// install never depends on the user's own Node or JDK.
//   runtime.node  — official Node 22 LTS build, trimmed to the executable
//                   (`node.exe` / `bin/node`) + LICENSE at the pack root.
//   runtime.java  — Eclipse Temurin JRE 21, repackaged unchanged (top-level
//                   `jdk-…-jre/` stripped).
// Pins (version, per-target URL + sha256) live in servers.json → `runtimes`.

export const RUNTIMES = ['node', 'java'];

function assetFor(runtime, target, pins) {
  if (!TARGETS.includes(target)) throw new Error(`unknown target "${target}" (want ${TARGETS.join(' | ')})`);
  const pin = pins.runtimes[runtime];
  const asset = pin.assets[target];
  if (!asset) throw new Error(`no ${runtime} asset pinned for ${target}`);
  return { ...asset, version: pin.version };
}

/** `{ url, sha256, unpack, binary, version }` of the pinned Node build. */
export function nodeAssetFor(target, pins = loadServerPins()) {
  return assetFor('node', target, pins);
}

/** `{ url, sha256, unpack, binary, version, release }` of the pinned Temurin JRE. */
export function temurinAssetFor(target, pins = loadServerPins()) {
  return { ...assetFor('java', target, pins), release: pins.runtimes.java.release };
}

const LABELS = { node: 'Node.js 22 runtime', java: 'Java 21 runtime (Temurin JRE)' };

/** Manifest per the pack contract (`kind: "runtime"`). */
export function runtimeManifest(runtime, target, { pins = loadServerPins(), packRev = 1 } = {}) {
  if (!RUNTIMES.includes(runtime)) throw new Error(`unknown runtime "${runtime}" (want ${RUNTIMES.join(' | ')})`);
  const asset = runtime === 'node' ? nodeAssetFor(target, pins) : temurinAssetFor(target, pins);
  const upstream = { url: asset.url, sha256: asset.sha256, version: asset.version };
  if (asset.release) upstream.release = asset.release;
  return {
    schema: 1,
    id: `runtime.${runtime}`,
    kind: 'runtime',
    version: `${asset.version}-${packRev}`,
    label: LABELS[runtime],
    runtime,
    binary: asset.binary,
    upstream,
    target,
    minOrreryVersion: MIN_ORRERY_VERSION,
  };
}

export function runtimePackName(manifest) {
  return packName(manifest.id, manifest.version, manifest.target);
}

/** Upstream archive → flat file entries `[{ name, data, mode }]` (top-level
 *  directory stripped, symlinks dereferenced). */
export function unpackArchive(buf, unpack) {
  if (unpack === 'zip') return stripComponents(readZipEntries(buf), 1);
  if (unpack === 'tar.gz') return stripComponents(dereference(readTarGz(buf)), 1);
  throw new Error(`unsupported unpack "${unpack}"`);
}

/** What a runtime pack keeps out of the upstream tree: Node → the binary +
 *  LICENSE only; Java → everything. Throws when the binary is missing. */
export function selectRuntimeFiles(runtime, entries, binary) {
  const keep = runtime === 'node' ? entries.filter((e) => e.name === binary || e.name === 'LICENSE') : entries;
  if (!keep.some((e) => e.name === binary)) throw new Error(`upstream archive has no ${binary}`);
  return keep;
}

const byName = (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0); // byte order: locale-independent, deterministic zips

/** Zip entries for a pack: manifest first, then the files (sorted for a
 *  deterministic archive). Executable bits come from the tar entries. */
export function packEntries(manifest, files) {
  const sorted = [...files].sort(byName);
  return [
    { name: 'manifest.json', data: JSON.stringify(manifest, null, 2) + '\n' },
    ...sorted.map((f) => ({ name: f.name, data: f.data, ...(f.mode ? { mode: f.mode } : {}) })),
  ];
}

export async function buildRuntime(runtime, { target, packRev = 1, outDir, cacheDir, pins = loadServerPins() }) {
  const manifest = runtimeManifest(runtime, target, { pins, packRev });
  const asset = runtime === 'node' ? nodeAssetFor(target, pins) : temurinAssetFor(target, pins);
  const buf = await fetchAsset(asset, cacheDir);
  const files = selectRuntimeFiles(runtime, unpackArchive(buf, asset.unpack), asset.binary);
  mkdirSync(outDir, { recursive: true });
  const zipPath = join(outDir, runtimePackName(manifest));
  writeZip(zipPath, packEntries(manifest, files));
  console.log(`wrote ${zipPath} (${(statSync(zipPath).size / 1048576).toFixed(1)} MB, ${files.length} files)`);
  return zipPath;
}

// CLI: node scripts/extensions/build-runtimes.mjs --target windows-x86_64 [--only node,java]
//        [--rev 1] [--out dist-ext] [--build .ext-build]
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const a = parseArgs(process.argv.slice(2));
  const target = a.target ?? (process.platform === 'win32' ? 'windows-x86_64' : 'macos-aarch64');
  const only = !a.only || a.only === 'all' ? RUNTIMES : a.only.split(',').map((s) => s.trim().replace(/^runtime\./, ''));
  const outDir = resolve(a.out ?? 'dist-ext');
  const cacheDir = join(resolve(a.build ?? '.ext-build'), 'downloads');
  const packRev = Number(a.rev ?? 1);
  let failed = false;
  for (const runtime of only) {
    try {
      await buildRuntime(runtime, { target, packRev, outDir, cacheDir });
    } catch (e) {
      failed = true;
      console.error(`runtime.${runtime}: ${e.message}`);
    }
  }
  if (failed) process.exit(1);
}
