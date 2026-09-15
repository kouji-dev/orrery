import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import {
  closeSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { writeZip } from './zip.mjs';

// Builds one grammar pack (`dist-ext/<id>-<version>-<target>.zip`) from the
// pinned upstream tree-sitter tarball: compile parser.c (+ scanner) into a
// shared library, copy the tags/locals queries, write manifest.json, zip.
// Pure helpers are exported for the vitest spec; the CLI lives under the
// pathToFileURL guard at the bottom (see stamp-version.mjs).

const HERE = dirname(fileURLToPath(import.meta.url));
export const PINS_FILE = join(HERE, 'grammars.json');
export const MIN_ORRERY_VERSION = '0.24.0';
export const TARGETS = ['windows-x86_64', 'macos-aarch64'];

export function loadPins(file = PINS_FILE) {
  return JSON.parse(readFileSync(file, 'utf8'));
}

/** `--lang java` matches `pin.language` (or the id); `all` / a csv resolve to many. */
export function selectPins(pins, lang) {
  if (!lang || lang === 'all') return pins;
  const wanted = lang.split(',').map((s) => s.trim()).filter(Boolean);
  return wanted.map((l) => {
    const pin = pins.find((p) => p.language === l || p.id === l || p.id === `grammar.${l}`);
    if (!pin) throw new Error(`no grammar pin for "${l}" in ${PINS_FILE}`);
    return pin;
  });
}

export function tarballUrl(pin) {
  return `https://github.com/${pin.repo}/archive/refs/tags/${pin.tag}.tar.gz`;
}

/** `v0.23.5` + rev 2 → `0.23.5-2`. */
export function packVersion(tag, packRev = 1) {
  return `${tag.replace(/^v/, '')}-${packRev}`;
}

export function packName(id, version, target) {
  return `${id}-${version}-${target}.zip`;
}

/** Inverse of packName. The version (`<semver>-<rev>`) is the only part of the
 *  name with a fixed shape, so anchor on it: ids and targets both carry dashes. */
export function parsePackName(file) {
  // 3 or 4 numeric parts: Temurin's `21.0.12.1`.
  const m = /^(.+?)-(\d+(?:\.\d+){2,3}-\d+)-(.+)\.zip$/.exec(file);
  return m ? { id: m[1], version: m[2], target: m[3] } : null;
}

export function libraryName(language, target) {
  const ext = target.startsWith('windows-') ? 'dll' : target.startsWith('macos-') ? 'dylib' : 'so';
  return `tree_sitter_${language}.${ext}`;
}

/** Grammar ABI. The 0.23–0.25 generators stamp `#define LANGUAGE_VERSION N` at
 *  the top of parser.c (parser.h carries no version define); the older
 *  `TREE_SITTER_LANGUAGE_VERSION` in parser.h is kept as a fallback. */
export function parseAbi(parserC, parserH = '') {
  const c = /#define\s+LANGUAGE_VERSION\s+(\d+)/.exec(parserC);
  if (c) return Number(c[1]);
  const h = /#define\s+TREE_SITTER_LANGUAGE_VERSION\s+(\d+)/.exec(parserH);
  if (h) return Number(h[1]);
  return null;
}

/** First 4 KiB of a file — parser.c is megabytes, the define is near the top. */
function head(path) {
  if (!existsSync(path)) return '';
  const buf = Buffer.alloc(4096);
  const fd = openSync(path, 'r');
  try {
    const n = readSync(fd, buf, 0, buf.length, 0);
    return buf.toString('utf8', 0, n);
  } finally {
    closeSync(fd);
  }
}

export function readAbi(srcDir) {
  return parseAbi(head(join(srcDir, 'parser.c')), head(join(srcDir, 'tree_sitter', 'parser.h')));
}

const LABELS = { typescript: 'TypeScript', tsx: 'TSX', javascript: 'JavaScript' };
function label(language) {
  return `${LABELS[language] ?? language.charAt(0).toUpperCase() + language.slice(1)} grammar`;
}

/** Manifest per the plan's "Grammar manifest" schema. `abi` is read from
 *  `srcDir` when given; the spec passes it inline. */
export function buildManifest(pin, { target, packRev = 1, srcDir, abi } = {}) {
  if (!TARGETS.includes(target)) {
    throw new Error(`unknown target "${target}" (want ${TARGETS.join(' | ')})`);
  }
  return {
    schema: 1,
    id: pin.id,
    kind: 'grammar',
    version: packVersion(pin.tag, packRev),
    label: label(pin.language),
    language: pin.language,
    monacoLanguage: pin.monacoLanguage,
    fileExtensions: [...pin.fileExtensions],
    symbol: pin.symbol,
    abi: abi ?? (srcDir ? readAbi(srcDir) : null),
    library: libraryName(pin.language, target),
    queries: { tags: 'queries/tags.scm', locals: pin.hasLocals ? 'queries/locals.scm' : null },
    upstream: { repo: pin.repo, tag: pin.tag },
    target,
    minOrreryVersion: MIN_ORRERY_VERSION,
  };
}

/** Compiler invocation for one target: cl.exe on Windows, clang elsewhere.
 *  `srcDir` is the grammar's `src/` (where parser.c lives). Returns
 *  `{ cmd, args }`; join with spaces to read the command line. */
export function compileArgs(target, { srcDir, hasScanner = false, hasScannerCc = false, out }) {
  const sources = [join(srcDir, 'parser.c')];
  if (hasScannerCc) sources.push(join(srcDir, 'scanner.cc'));
  else if (hasScanner) sources.push(join(srcDir, 'scanner.c'));
  if (target.startsWith('windows-')) {
    return { cmd: 'cl', args: ['/LD', '/O2', `/I${srcDir}`, ...sources, `/Fe:${out}`] };
  }
  const args = ['-shared', '-O2', '-fPIC', `-I${srcDir}`, ...sources, '-o', out];
  if (hasScannerCc) args.push('-lc++');
  return { cmd: 'clang', args };
}

export function sha256(buf) {
  return createHash('sha256').update(buf).digest('hex');
}

/** `--k v` pairs; a `--flag` followed by another `--x` (or nothing) is `true`. */
export function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) continue;
    const next = argv[i + 1];
    if (next === undefined || next.startsWith('--')) out[a.slice(2)] = true;
    else {
      out[a.slice(2)] = next;
      i++;
    }
  }
  return out;
}

// ------------------------------------------------------------------ pipeline

async function download(url, dest) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`${url} → HTTP ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  mkdirSync(dirname(dest), { recursive: true });
  writeFileSync(dest, buf);
  return buf;
}

/** Download + verify + extract the pinned tarball; returns the repo root
 *  (`.ext-build/<lang>/<repo>-<tag>/`). Idempotent: skips what exists. */
async function fetchSource(pin, buildDir) {
  mkdirSync(buildDir, { recursive: true });
  const tarball = join(buildDir, `${pin.tag}.tar.gz`);
  let buf;
  if (existsSync(tarball)) {
    buf = readFileSync(tarball);
    console.log(`using cached ${tarball}`);
  } else {
    console.log(`downloading ${tarballUrl(pin)}`);
    buf = await download(tarballUrl(pin), tarball);
  }
  const digest = sha256(buf);
  if (pin.sha256 && pin.sha256 !== digest) {
    rmSync(tarball);
    throw new Error(`sha256 mismatch for ${pin.id} tarball: pinned ${pin.sha256}, got ${digest}`);
  }
  if (!pin.sha256) {
    console.warn(`${pin.id}: no sha256 pinned — set "sha256": "${digest}" in grammars.json`);
  }

  const root = join(buildDir, `${pin.repo.split('/')[1]}-${pin.tag.replace(/^v/, '')}`);
  if (!existsSync(join(root, 'queries'))) {
    // Relative path + cwd: GNU tar (Git Bash on PATH) reads `C:\…` as a remote host.
    const tar = spawnSync('tar', ['-xzf', `${pin.tag}.tar.gz`], { cwd: buildDir, stdio: 'inherit' });
    if (tar.status !== 0) throw new Error(`tar failed (${tar.status ?? tar.error})`);
  }
  if (!existsSync(root)) throw new Error(`extracted tarball did not produce ${root}`);
  return root;
}

/** On Windows `cl` is only on PATH inside a Developer Command Prompt. Find
 *  vcvars64.bat via vswhere and wrap the compile in it so a plain shell works
 *  too; the wrapped command line is printed so it can be replayed by hand. */
function findVcvars() {
  if (spawnSync('where', ['cl'], { stdio: 'ignore' }).status === 0) return null;
  const vswhere = join(
    process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)',
    'Microsoft Visual Studio',
    'Installer',
    'vswhere.exe',
  );
  if (!existsSync(vswhere)) {
    throw new Error('cl.exe not on PATH and vswhere.exe not found — install the MSVC Build Tools (C++ workload)');
  }
  const r = spawnSync(
    vswhere,
    ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'],
    { encoding: 'utf8' },
  );
  const install = (r.stdout ?? '').trim().split(/\r?\n/)[0];
  const vcvars = install && join(install, 'VC', 'Auxiliary', 'Build', 'vcvars64.bat');
  if (!vcvars || !existsSync(vcvars)) {
    throw new Error('cl.exe not on PATH and no vcvars64.bat found — install the "Desktop development with C++" workload');
  }
  return vcvars;
}

const quote = (s) => (/[\s"]/.test(s) ? `"${s.replace(/"/g, '\\"')}"` : s);

function compile(target, opts, cwd) {
  const { cmd, args } = compileArgs(target, opts);
  const line = [cmd, ...args.map(quote)].join(' ');
  let result;
  const vcvars = process.platform === 'win32' && cmd === 'cl' ? findVcvars() : null;
  if (vcvars) {
    const wrapped = `"${vcvars}" >nul && ${line}`;
    console.log(`$ cmd /d /s /c ${wrapped}`);
    result = spawnSync('cmd.exe', ['/d', '/s', '/c', `"${wrapped}"`], {
      cwd,
      stdio: 'inherit',
      windowsVerbatimArguments: true,
    });
  } else {
    console.log(`$ ${line}`);
    result = spawnSync(cmd, args, { cwd, stdio: 'inherit', shell: process.platform === 'win32' });
  }
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${cmd} exited with ${result.status}`);
}

/** Repo-local override (`scripts/extensions/queries/<lang>/<name>`) wins over
 *  upstream `queries/<name>`. tree-sitter-typescript keeps one root `queries/`
 *  for both subdirs, so lookups go to the repo root, not the grammar dir. */
function pickQuery(pin, repoRoot, name) {
  const override = join(HERE, 'queries', pin.language, name);
  if (existsSync(override)) return override;
  const upstream = join(repoRoot, 'queries', name);
  return existsSync(upstream) ? upstream : null;
}

export async function buildPack(pin, { target, packRev = 1, outDir, src, buildRoot }) {
  const buildDir = join(buildRoot, pin.language);
  const repoRoot = src ? resolve(src) : await fetchSource(pin, buildDir);
  const grammarDir = pin.subdir ? join(repoRoot, pin.subdir) : repoRoot;
  const srcDir = join(grammarDir, 'src');
  if (!existsSync(join(srcDir, 'parser.c'))) throw new Error(`no parser.c in ${srcDir}`);

  const manifest = buildManifest(pin, { target, packRev, srcDir });
  const stage = join(buildDir, `stage-${target}`);
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(join(stage, 'queries'), { recursive: true });

  const libOut = join(stage, manifest.library);
  compile(
    target,
    {
      srcDir,
      hasScanner: existsSync(join(srcDir, 'scanner.c')),
      hasScannerCc: existsSync(join(srcDir, 'scanner.cc')),
      out: libOut,
    },
    stage,
  );
  if (!existsSync(libOut)) throw new Error(`compiler produced no ${libOut}`);

  const tags = pickQuery(pin, repoRoot, 'tags.scm');
  if (!tags) {
    throw new Error(`${pin.id}: no queries/tags.scm upstream and no override in scripts/extensions/queries/${pin.language}/`);
  }
  copyFileSync(tags, join(stage, 'queries', 'tags.scm'));
  if (pin.hasLocals) {
    const locals = pickQuery(pin, repoRoot, 'locals.scm');
    if (!locals) throw new Error(`${pin.id}: hasLocals is set but no queries/locals.scm found`);
    copyFileSync(locals, join(stage, 'queries', 'locals.scm'));
  }
  const manifestJson = JSON.stringify(manifest, null, 2) + '\n';
  writeFileSync(join(stage, 'manifest.json'), manifestJson);

  const entries = [
    { name: 'manifest.json', data: manifestJson },
    { name: manifest.library, data: readFileSync(libOut) },
    { name: 'queries/tags.scm', data: readFileSync(join(stage, 'queries', 'tags.scm')) },
  ];
  if (pin.hasLocals) {
    entries.push({ name: 'queries/locals.scm', data: readFileSync(join(stage, 'queries', 'locals.scm')) });
  }

  mkdirSync(outDir, { recursive: true });
  const zipPath = join(outDir, packName(manifest.id, manifest.version, target));
  writeZip(zipPath, entries);
  console.log(`wrote ${zipPath} (${statSync(zipPath).size} bytes, abi ${manifest.abi})`);
  return zipPath;
}

// CLI: node scripts/extensions/build-grammar.mjs --lang java|all|a,b --target windows-x86_64
//        [--rev 1] [--out dist-ext] [--src <grammar repo root>] [--build .ext-build]
// pathToFileURL: see stamp-version.mjs — the bare-string check fails on Windows.
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const a = parseArgs(process.argv.slice(2));
  const target = a.target ?? (process.platform === 'win32' ? 'windows-x86_64' : 'macos-aarch64');
  const pins = selectPins(loadPins(), a.lang ?? 'all');
  if (a.src && pins.length !== 1) {
    console.error('--src applies to exactly one --lang');
    process.exit(1);
  }
  const outDir = resolve(a.out ?? 'dist-ext');
  const buildRoot = resolve(a.build ?? '.ext-build');
  const packRev = Number(a.rev ?? 1);
  let failed = false;
  for (const pin of pins) {
    try {
      await buildPack(pin, { target, packRev, outDir, src: a.src, buildRoot });
    } catch (e) {
      failed = true;
      console.error(`${pin.id}: ${e.message}`);
    }
  }
  if (failed) process.exit(1);
}
