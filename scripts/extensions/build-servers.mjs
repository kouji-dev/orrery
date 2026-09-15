import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { TARGETS, packName, parseArgs } from './build-grammar.mjs';
import { fetchAsset } from './fetch.mjs';
import { dereference, readTarGz } from './tar.mjs';
import { readZipEntries, writeZip } from './zip.mjs';

// Server packs ship the language server itself (self-contained: no npm / go /
// cargo / JDK on the user's box). `servers/<name>.json` is the manifest
// template (launch recipe, placeholders, detect fallback); `servers.json`
// pins the upstream versions, URLs and sha256s. Per pack:
//   node packs (ts-ls, pyright) — `npm install --prefix <stage>/server …` on
//     the BUILD machine, then `server/node_modules` zipped verbatim (trimmed of
//     docs/maps/tests, symlink-free, `.bin` shims dropped). target `any`.
//   gopls — `go install golang.org/x/tools/gopls@<pin>` with GOBIN → stage;
//     host target only (go refuses cross-compiled installs into GOBIN).
//   rust-analyzer — upstream release asset, verified, unzipped/gunzipped.
//   jdtls — upstream tar.gz, verified, repacked as a zip (flat layout kept).
// Placeholders the host substitutes in `launch.args` / initializationOptions:
//   ${extDir}            the pack's install directory
//   ${workspaceStorage}  per-project scratch dir the host owns
//   ${projectRoot}       the open project's root
//   ${jdtlsConfig}       jdtls' Equinox config dir for the running platform
//                        (see `placeholders.jdtlsConfig` in servers/jdtls.json)

const HERE = dirname(fileURLToPath(import.meta.url));
export const SERVERS_DIR = join(HERE, 'servers');
export const SERVER_PINS_FILE = join(HERE, 'servers.json');

export function loadServerPins(file = SERVER_PINS_FILE) {
  return JSON.parse(readFileSync(file, 'utf8'));
}

export function loadServerManifests(dir = SERVERS_DIR) {
  return readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((f) => JSON.parse(readFileSync(join(dir, f), 'utf8')));
}

/** `--only server.jdtls,server.gopls` (ids or bare names); `all` = everything. */
export function selectServers(manifests, only) {
  if (!only || only === 'all') return manifests;
  const wanted = only.split(',').map((s) => s.trim()).filter(Boolean);
  return wanted.map((w) => {
    const m = manifests.find((s) => s.id === w || s.id === `server.${w}`);
    if (!m) throw new Error(`no server manifest for "${w}" in ${SERVERS_DIR}`);
    return m;
  });
}

/** `any` packs are platform-independent (JS, jars); `per-target` templates
 *  (native binaries) need one zip per target. */
export function isTargetSpecific(template) {
  return template.target !== 'any';
}

export function hostTarget(platform = process.platform, arch = process.arch) {
  if (platform === 'win32' && arch === 'x64') return 'windows-x86_64';
  if (platform === 'darwin' && arch === 'arm64') return 'macos-aarch64';
  return null;
}

/** Template + pins → the manifest that goes into the zip. `version` is
 *  `<pinned upstream>-<packRev>`; the template's version must agree with the
 *  pin (drift guard). `target` is filled for platform-specific packs. */
export function serverManifest(template, pins, { target, packRev = 1 }) {
  const pin = pins.servers[template.id];
  if (!pin) throw new Error(`${template.id}: no pin in servers.json`);
  const templateUpstream = String(template.version).replace(/-\d+$/, '');
  if (templateUpstream !== pin.version) {
    throw new Error(`${template.id}: template version ${template.version} disagrees with pinned ${pin.version}`);
  }
  const t = isTargetSpecific(template) ? target : 'any';
  if (t !== 'any' && !TARGETS.includes(t)) throw new Error(`unknown target "${t}" (want ${TARGETS.join(' | ')})`);
  const out = { ...template, version: `${pin.version}-${packRev}`, target: t };
  // Native binaries: the template names the bare entry, Windows gets `.exe`.
  if (t.startsWith('windows-') && template.launch?.runtime === null) {
    out.launch = { ...template.launch, entry: `${template.launch.entry}.exe` };
  }
  return out;
}

export function serverPackName(manifest) {
  return packName(manifest.id, manifest.version, manifest.target ?? 'any');
}

// ------------------------------------------------------------------ node packs

const DROP_DIRS = new Set(['test', 'tests', '__tests__', 'docs', 'doc', '.bin', '.github']);
const DROP_FILES = new Set(['.package-lock.json']); // npm's hidden lockfile
const DROP_EXT = /\.(md|markdown|map)$/i;

/** Keep this `server/node_modules/…` file in the pack? Drops docs, source
 *  maps, test dirs and npm's `.bin` shims (symlinks on unix). */
export function keepNodeFile(rel) {
  const parts = rel.split('/');
  if (parts.some((p) => DROP_DIRS.has(p))) return false;
  const base = parts[parts.length - 1];
  return !DROP_FILES.has(base) && !DROP_EXT.test(base);
}

export function npmInstallArgs(prefix, npmPins) {
  const specs = Object.entries(npmPins).map(([name, version]) => `${name}@${version}`);
  return ['install', '--prefix', prefix, '--no-audit', '--no-fund', '--omit=dev', '--no-package-lock', ...specs];
}

export function goInstallArgs(goPin) {
  return ['install', `${goPin.module}@${goPin.version}`];
}

function onPath(cmd) {
  const probe = process.platform === 'win32' ? 'where' : 'which';
  return spawnSync(probe, [cmd], { stdio: 'ignore' }).status === 0;
}

/** Regular files under `root` as `[{ name (posix, relative), abs, mode }]`;
 *  symlinks are skipped (a zip cannot carry them). */
export function walkFiles(root, rel = '') {
  const out = [];
  const dir = rel ? join(root, rel) : root;
  for (const ent of readdirSync(dir, { withFileTypes: true }).sort(byName)) {
    const name = rel ? `${rel}/${ent.name}` : ent.name;
    const st = lstatSync(join(root, name));
    if (st.isSymbolicLink()) continue;
    if (st.isDirectory()) out.push(...walkFiles(root, name));
    else if (st.isFile()) out.push({ name, abs: join(root, name), mode: st.mode & 0o777 });
  }
  return out;
}

function run(cmd, args, opts = {}) {
  console.log(`$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, { stdio: 'inherit', shell: process.platform === 'win32', ...opts });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`${cmd} exited with ${r.status}`);
}

function stageDir(buildRoot, manifest) {
  const dir = join(buildRoot, 'servers', manifest.id.replace(/^server\./, ''), `stage-${manifest.target}`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  return dir;
}

async function nodeFiles(manifest, pin, buildRoot) {
  if (!onPath('npm')) return { skip: 'npm not on PATH (build-time tool)' };
  const stage = stageDir(buildRoot, manifest);
  const prefix = join(stage, 'server');
  mkdirSync(prefix, { recursive: true });
  run('npm', npmInstallArgs(prefix, pin.npm));
  const modules = join(prefix, 'node_modules');
  if (!existsSync(modules)) throw new Error(`npm install produced no ${modules}`);
  const files = walkFiles(modules)
    .filter((f) => keepNodeFile(f.name))
    .map((f) => ({ name: `server/node_modules/${f.name}`, data: readFileSync(f.abs) }));
  const entry = manifest.launch?.entry;
  if (entry && !files.some((f) => f.name === entry)) throw new Error(`launch entry ${entry} not in the installed tree`);
  return { files };
}

async function goplsFiles(manifest, pin, buildRoot) {
  const host = hostTarget();
  if (manifest.target !== host) return { skip: `go cannot install a cross-compiled binary into GOBIN (host is ${host ?? process.platform})` };
  if (!onPath('go')) return { skip: 'go not on PATH (build-time tool)' };
  const stage = stageDir(buildRoot, manifest);
  run('go', goInstallArgs(pin.go), { env: { ...process.env, GOBIN: stage, GOFLAGS: '-trimpath' } });
  const entry = manifest.launch.entry;
  const abs = join(stage, entry);
  if (!existsSync(abs)) throw new Error(`go install produced no ${abs}`);
  return { files: [{ name: entry, data: readFileSync(abs), mode: 0o755 }] };
}

async function rustAnalyzerFiles(manifest, pin, cacheDir) {
  const asset = pin.assets[manifest.target];
  if (!asset) return { skip: `no rust-analyzer asset pinned for ${manifest.target}` };
  const buf = await fetchAsset(asset, cacheDir);
  const entry = manifest.launch.entry;
  if (asset.unpack === 'zip') {
    const hit = readZipEntries(buf).find((e) => e.name === asset.entry);
    if (!hit) throw new Error(`${asset.url}: no ${asset.entry} inside`);
    return { files: [{ name: entry, data: hit.data, mode: 0o755 }] };
  }
  if (asset.unpack === 'gz') return { files: [{ name: entry, data: gunzipSync(buf), mode: 0o755 }] };
  throw new Error(`unsupported unpack "${asset.unpack}"`);
}

async function jdtlsFiles(manifest, pin, cacheDir) {
  const buf = await fetchAsset(pin.asset, cacheDir);
  const files = dereference(readTarGz(buf)).map((e) => ({ name: e.name, data: e.data, mode: e.mode }));
  const launcher = /^plugins\/org\.eclipse\.equinox\.launcher_.*\.jar$/;
  if (!files.some((f) => launcher.test(f.name))) throw new Error(`${pin.asset.url}: no Equinox launcher jar inside`);
  for (const cfg of Object.values(manifest.placeholders?.jdtlsConfig ?? {})) {
    if (!files.some((f) => f.name.startsWith(`${cfg}/`))) throw new Error(`${pin.asset.url}: no ${cfg}/ inside`);
  }
  return { files };
}

const BUILDERS = {
  'server.typescript-language-server': (m, pin, ctx) => nodeFiles(m, pin, ctx.buildRoot),
  'server.pyright': (m, pin, ctx) => nodeFiles(m, pin, ctx.buildRoot),
  'server.gopls': (m, pin, ctx) => goplsFiles(m, pin, ctx.buildRoot),
  'server.rust-analyzer': (m, pin, ctx) => rustAnalyzerFiles(m, pin, ctx.cacheDir),
  'server.jdtls': (m, pin, ctx) => jdtlsFiles(m, pin, ctx.cacheDir),
};

const byName = (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0); // byte order: locale-independent, deterministic zips

/** Zip entries for a pack: manifest first, files sorted (deterministic zip). */
export function packEntries(manifest, files) {
  const sorted = [...files].sort(byName);
  return [
    { name: 'manifest.json', data: JSON.stringify(manifest, null, 2) + '\n' },
    ...sorted.map((f) => ({ name: f.name, data: f.data, ...(f.mode ? { mode: f.mode } : {}) })),
  ];
}

/** Builds one pack; resolves to `{ zip }` or `{ skipped: reason }`. */
export async function buildServer(template, { target, packRev = 1, outDir, buildRoot, pins = loadServerPins() }) {
  const manifest = serverManifest(template, pins, { target, packRev });
  const builder = BUILDERS[manifest.id];
  if (!builder) throw new Error(`${manifest.id}: no builder`);
  const r = await builder(manifest, pins.servers[manifest.id], { buildRoot, cacheDir: join(buildRoot, 'downloads') });
  if (r.skip) {
    console.warn(`skip ${manifest.id} (${manifest.target}): ${r.skip}`);
    return { skipped: r.skip };
  }
  mkdirSync(outDir, { recursive: true });
  const zip = join(outDir, serverPackName(manifest));
  writeZip(zip, packEntries(manifest, r.files));
  console.log(`wrote ${zip} (${(statSync(zip).size / 1048576).toFixed(1)} MB, ${r.files.length} files)`);
  return { zip };
}

// CLI: node scripts/extensions/build-servers.mjs --target windows-x86_64 [--only server.jdtls,…]
//        [--skip-any] [--rev 1] [--out dist-ext] [--build .ext-build]
// `--skip-any`: platform-specific packs only (CI builds the `any` packs once).
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const a = parseArgs(process.argv.slice(2));
  const target = a.target ?? hostTarget() ?? 'windows-x86_64';
  const outDir = resolve(a.out ?? 'dist-ext');
  const buildRoot = resolve(a.build ?? '.ext-build');
  const packRev = Number(a.rev ?? 1);
  let failed = false;
  for (const template of selectServers(loadServerManifests(), a.only ?? 'all')) {
    if (a['skip-any'] && !isTargetSpecific(template)) continue;
    try {
      await buildServer(template, { target, packRev, outDir, buildRoot });
    } catch (e) {
      failed = true;
      console.error(`${template.id}: ${e.message}`);
    }
  }
  if (failed) process.exit(1);
}
