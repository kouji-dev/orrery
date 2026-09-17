import { gunzipSync } from 'node:zlib';

// Minimal tar reader (POSIX ustar + GNU long names + pax headers): enough to
// repackage the upstream tarballs (Node, Temurin, jdtls) into packs without a
// tar dependency. Returns plain entries; symlinks are kept as records and can
// be dereferenced with `dereference()` so a zip (no symlinks) carries the
// target's bytes instead.

const BLOCK = 512;

function str(buf, off, len) {
  const s = buf.toString('utf8', off, off + len);
  const nul = s.indexOf('\0');
  return nul === -1 ? s : s.slice(0, nul);
}

/** Octal text or GNU base-256 (high bit set in the first byte). */
export function parseNumeric(buf, off, len) {
  if (buf[off] & 0x80) {
    let n = 0;
    for (let i = 1; i < len; i++) n = n * 256 + buf[off + i];
    return n;
  }
  const s = str(buf, off, len).trim();
  return s ? parseInt(s, 8) : 0;
}

/** Parse pax extended header records (`<len> <key>=<value>\n`). */
export function parsePax(buf) {
  const out = {};
  let pos = 0;
  const text = buf.toString('utf8');
  while (pos < text.length) {
    const sp = text.indexOf(' ', pos);
    if (sp === -1) break;
    const len = Number(text.slice(pos, sp));
    if (!len) break;
    const rec = text.slice(sp + 1, pos + len - 1);
    const eq = rec.indexOf('=');
    if (eq !== -1) out[rec.slice(0, eq)] = rec.slice(eq + 1);
    pos += len;
  }
  return out;
}

/** `[{ name, type: 'file'|'dir'|'symlink'|'other', mode, size, data, linkname }]`.
 *  `data` is a Buffer for files (a subarray of `buf` — no copy). */
export function readTar(buf) {
  const out = [];
  let pos = 0;
  let longName = null;
  let longLink = null;
  let pax = null;
  while (pos + BLOCK <= buf.length) {
    if (buf[pos] === 0 && buf.subarray(pos, pos + BLOCK).every((b) => b === 0)) break; // end-of-archive
    const h = buf.subarray(pos, pos + BLOCK);
    let name = str(h, 0, 100);
    const mode = parseNumeric(h, 100, 8) & 0o7777;
    const size = parseNumeric(h, 124, 12);
    const typeflag = String.fromCharCode(h[156] || 0x30);
    let linkname = str(h, 157, 100);
    const magic = str(h, 257, 6);
    const prefix = magic.startsWith('ustar') ? str(h, 345, 155) : '';
    if (prefix) name = `${prefix}/${name}`;
    const dataStart = pos + BLOCK;
    const data = buf.subarray(dataStart, dataStart + size);
    pos = dataStart + Math.ceil(size / BLOCK) * BLOCK;

    if (typeflag === 'L') {
      longName = str(data, 0, data.length);
      continue;
    }
    if (typeflag === 'K') {
      longLink = str(data, 0, data.length);
      continue;
    }
    if (typeflag === 'x') {
      pax = parsePax(data);
      continue;
    }
    if (typeflag === 'g') continue; // global pax header — nothing we use

    if (longName) name = longName;
    if (longLink) linkname = longLink;
    if (pax?.path) name = pax.path;
    if (pax?.linkpath) linkname = pax.linkpath;
    longName = longLink = pax = null;

    const type =
      typeflag === '0' || typeflag === '\0' || typeflag === '7'
        ? 'file'
        : typeflag === '5'
          ? 'dir'
          : typeflag === '2'
            ? 'symlink'
            : 'other';
    name = name.replace(/^\.\//, '').replace(/\/+$/, '');
    if (type === 'dir' || name === '' || name === '.') continue;
    out.push({ name, type, mode, size, data: type === 'file' ? data : null, linkname: type === 'symlink' ? linkname : null });
  }
  return out;
}

export function readTarGz(buf) {
  return readTar(gunzipSync(buf));
}

function dirname(p) {
  const i = p.lastIndexOf('/');
  return i === -1 ? '' : p.slice(0, i);
}

function normalize(p) {
  const parts = [];
  for (const seg of p.split('/')) {
    if (!seg || seg === '.') continue;
    if (seg === '..') parts.pop();
    else parts.push(seg);
  }
  return parts.join('/');
}

/** Replace every symlink entry by a copy of its target's bytes (relative
 *  links resolve against the link's directory; chains are followed). Links
 *  whose target is not in the archive are dropped. Returns file entries only. */
export function dereference(entries) {
  const files = new Map(entries.filter((e) => e.type === 'file').map((e) => [e.name, e]));
  const links = new Map(entries.filter((e) => e.type === 'symlink').map((e) => [e.name, e]));
  const resolveTarget = (link, depth = 0) => {
    if (depth > 16) return null;
    const target = link.linkname.startsWith('/') ? normalize(link.linkname) : normalize(`${dirname(link.name)}/${link.linkname}`);
    if (files.has(target)) return files.get(target);
    if (links.has(target)) return resolveTarget(links.get(target), depth + 1);
    return null;
  };
  const out = [...files.values()];
  for (const link of links.values()) {
    const target = resolveTarget(link);
    if (target) out.push({ ...target, name: link.name, linkname: null });
  }
  return out;
}

/** Drop the first `n` path segments (`tar --strip-components`); entries with
 *  fewer segments are dropped. */
export function stripComponents(entries, n) {
  if (!n) return entries;
  const out = [];
  for (const e of entries) {
    const parts = e.name.split('/');
    if (parts.length <= n) continue;
    out.push({ ...e, name: parts.slice(n).join('/') });
  }
  return out;
}
