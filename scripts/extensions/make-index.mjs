import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs, parsePackName, sha256 } from './build-grammar.mjs';
import { readZipEntries } from './zip.mjs';

// Builds the registry `index.json` from a directory of pack zips, merging the
// previously published index so packs not rebuilt this run keep their entries.

export const INDEX_SCHEMA = 1;

export function releaseTag(id, version) {
  return `ext-${id}-v${version}`;
}

/** `https://github.com/<repo>/releases/download/ext-<id>-v<version>/<file>` */
export function artifactUrl(repo, id, version, file) {
  return `https://github.com/${repo}/releases/download/${releaseTag(id, version)}/${file}`;
}

/** Pack-level fields of an index entry, derived from a manifest. `requires`
 *  (runtime pack ids a server needs) is mirrored so the panel can say
 *  "includes Node runtime" before the manifest is downloaded. */
export function packEntry(manifest) {
  const entry = {
    id: manifest.id,
    kind: manifest.kind,
    version: manifest.version,
    description: manifest.label,
    languages: manifest.kind === 'grammar' ? [manifest.language] : [...(manifest.languages ?? [])],
  };
  if (manifest.kind === 'grammar') entry.abi = manifest.abi;
  if (manifest.kind === 'runtime') entry.runtime = manifest.runtime;
  if (Array.isArray(manifest.requires)) entry.requires = [...manifest.requires];
  entry.minOrreryVersion = manifest.minOrreryVersion;
  return entry;
}

/** Which `targets` key an artifact lands under: the manifest's target, or
 *  `any` for a (legacy) server manifest without one. */
export function artifactTarget(manifest) {
  return manifest.target ?? (manifest.kind === 'server' ? 'any' : undefined);
}

/** `artifacts`: `[{ file, manifest, sha256, size, path? }]` (one per zip).
 *  Merge rule: an incoming id at the same version adds/overwrites its targets
 *  (a partial rebuild keeps the other platform); a new version replaces the
 *  pack entry wholesale; untouched packs from `prev` survive. Sorted by id.
 *  `local`: `file:///` URLs (dev registry) instead of GitHub release URLs. */
export function buildIndex(prev, artifacts, { repo, generated, local = false }) {
  const packs = new Map();
  for (const p of prev?.packs ?? []) packs.set(p.id, structuredClone(p));

  for (const art of artifacts) {
    const { manifest } = art;
    const target = artifactTarget(manifest);
    if (!target) throw new Error(`${art.file}: manifest has no target`);
    const existing = packs.get(manifest.id);
    const entry =
      existing && existing.version === manifest.version
        ? existing
        : { ...packEntry(manifest), targets: {} };
    entry.targets[target] = {
      url: local ? pathToFileURL(resolve(art.path ?? art.file)).href : artifactUrl(repo, manifest.id, manifest.version, art.file),
      sha256: art.sha256,
      size: art.size,
      sizeBytes: art.size,
    };
    packs.set(manifest.id, entry);
  }

  return {
    schema: INDEX_SCHEMA,
    generated,
    packs: [...packs.values()].sort((a, b) => a.id.localeCompare(b.id)),
  };
}

/** Read every `*.zip` in `dir` into an artifact record (manifest from inside
 *  the zip, sha256 + size from the file). Non-pack zips are skipped. */
export function readArtifacts(dir) {
  const out = [];
  for (const file of readdirSync(dir).sort()) {
    if (!file.endsWith('.zip') || !parsePackName(file)) continue;
    const path = join(dir, file);
    const buf = readFileSync(path);
    const entry = readZipEntries(buf).find((e) => e.name === 'manifest.json');
    if (!entry) throw new Error(`${file}: no manifest.json inside`);
    out.push({
      file,
      path,
      manifest: JSON.parse(entry.data.toString('utf8')),
      sha256: sha256(buf),
      size: statSync(path).size,
    });
  }
  return out;
}

/** Group artifacts by release tag — the publish job creates one GitHub
 *  release per pack version and uploads every target's zip to it. */
export function releasePlan(artifacts) {
  const byTag = new Map();
  for (const a of artifacts) {
    const tag = releaseTag(a.manifest.id, a.manifest.version);
    if (!byTag.has(tag)) byTag.set(tag, { tag, id: a.manifest.id, version: a.manifest.version, files: [] });
    byTag.get(tag).files.push(a.file);
  }
  return [...byTag.values()].sort((a, b) => a.tag.localeCompare(b.tag));
}

// CLI: node scripts/extensions/make-index.mjs --dir dist-ext --out dist-ext/index.json
//        --repo kouji-dev/orrery-releases [--prev prev.json] [--local] [--releases releases.json]
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const a = parseArgs(process.argv.slice(2));
  const dir = resolve(a.dir ?? 'dist-ext');
  const out = resolve(a.out ?? join(dir, 'index.json'));
  const repo = a.repo ?? 'kouji-dev/orrery-releases';
  const prev = a.prev && existsSync(a.prev) ? JSON.parse(readFileSync(a.prev, 'utf8')) : null;
  if (a.prev && !prev) console.warn(`no previous index at ${a.prev} — starting fresh`);
  const artifacts = readArtifacts(dir);
  const index = buildIndex(prev, artifacts, { repo, generated: new Date().toISOString(), local: a.local === true });
  writeFileSync(out, JSON.stringify(index, null, 2) + '\n');
  console.log(`wrote ${out}: ${index.packs.length} packs (${artifacts.length} new artifacts)`);
  if (a.releases) {
    writeFileSync(resolve(a.releases), JSON.stringify(releasePlan(artifacts), null, 2) + '\n');
    console.log(`wrote ${a.releases}`);
  }
}
