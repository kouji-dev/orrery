import { deflateRawSync, inflateRawSync, crc32 } from 'node:zlib';
import { writeFileSync } from 'node:fs';

// Minimal zip writer/reader (PKZIP app-note 4.4.x): local headers + central
// directory + EOCD, deflate (method 8) or store (method 0). No zip64, no
// encryption, no data descriptors. Enough for a pack of <100 small files;
// avoids pulling a zip dependency into the app's package.json.

const LOCAL_SIG = 0x04034b50;
const CENTRAL_SIG = 0x02014b50;
const EOCD_SIG = 0x06054b50;
const VERSION_NEEDED = 20;
// Fixed MS-DOS timestamp (2024-01-01 00:00:00) so a rebuild from identical
// inputs is byte-identical — sha256 in index.json stays stable.
const DOS_TIME = 0;
const DOS_DATE = ((2024 - 1980) << 9) | (1 << 5) | 1;

// "version made by" high byte 3 = UNIX: tells unzippers the external
// attributes carry a st_mode (executable bits survive on macOS).
const MADE_BY_UNIX = (3 << 8) | VERSION_NEEDED;
const S_IFREG = 0o100000;

/** `entries`: `[{ name, data: Buffer|string, store?, mode? }]`. Names use `/`.
 *  `mode` (e.g. 0o755) is stored as UNIX external attributes; without it the
 *  entry carries no permission bits (fine for Windows packs).
 *  Returns the zip file as a Buffer. */
export function zipBuffer(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const entry of entries) {
    const name = Buffer.from(entry.name.replace(/\\/g, '/'), 'utf8');
    const raw = Buffer.isBuffer(entry.data) ? entry.data : Buffer.from(entry.data, 'utf8');
    const deflated = entry.store ? raw : deflateRawSync(raw, { level: 9 });
    // Fall back to store when deflate doesn't help (already-compressed inputs).
    const useStore = entry.store || deflated.length >= raw.length;
    const body = useStore ? raw : deflated;
    const method = useStore ? 0 : 8;
    const crc = crc32(raw);

    const local = Buffer.alloc(30);
    local.writeUInt32LE(LOCAL_SIG, 0);
    local.writeUInt16LE(VERSION_NEEDED, 4);
    local.writeUInt16LE(0x0800, 6); // utf-8 names
    local.writeUInt16LE(method, 8);
    local.writeUInt16LE(DOS_TIME, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    locals.push(local, name, body);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(CENTRAL_SIG, 0);
    central.writeUInt16LE(entry.mode ? MADE_BY_UNIX : VERSION_NEEDED, 4);
    central.writeUInt16LE(VERSION_NEEDED, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(method, 10);
    central.writeUInt16LE(DOS_TIME, 12);
    central.writeUInt16LE(DOS_DATE, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(body.length, 20);
    central.writeUInt32LE(raw.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt16LE(0, 30); // extra
    central.writeUInt16LE(0, 32); // comment
    central.writeUInt16LE(0, 34); // disk
    central.writeUInt16LE(0, 36); // internal attrs
    central.writeUInt32LE(entry.mode ? ((S_IFREG | (entry.mode & 0o7777)) << 16) >>> 0 : 0, 38); // external attrs
    central.writeUInt32LE(offset, 42);
    centrals.push(central, name);

    offset += local.length + name.length + body.length;
  }
  const centralSize = centrals.reduce((n, b) => n + b.length, 0);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(EOCD_SIG, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralSize, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...locals, ...centrals, eocd]);
}

export function writeZip(file, entries) {
  writeFileSync(file, zipBuffer(entries));
}

/** Read every entry of a zip Buffer written by `zipBuffer` (or any zip64-free
 *  archive — sizes come from the central directory, so data descriptors are
 *  fine). Returns `[{ name, data, mode }]`; `mode` is the UNIX permission
 *  bits when the entry was made on UNIX, else null. Directory entries are
 *  skipped. */
export function readZipEntries(buf) {
  let eocd = buf.length - 22;
  while (eocd >= 0 && buf.readUInt32LE(eocd) !== EOCD_SIG) eocd--;
  if (eocd < 0) throw new Error('not a zip file (no end-of-central-directory record)');
  const count = buf.readUInt16LE(eocd + 10);
  let pos = buf.readUInt32LE(eocd + 16);
  const out = [];
  for (let i = 0; i < count; i++) {
    if (buf.readUInt32LE(pos) !== CENTRAL_SIG) throw new Error('bad central directory entry');
    const madeBy = buf.readUInt16LE(pos + 4) >> 8;
    const method = buf.readUInt16LE(pos + 10);
    const crc = buf.readUInt32LE(pos + 16);
    const compressed = buf.readUInt32LE(pos + 20);
    const nameLen = buf.readUInt16LE(pos + 28);
    const extraLen = buf.readUInt16LE(pos + 30);
    const commentLen = buf.readUInt16LE(pos + 32);
    const external = buf.readUInt32LE(pos + 38);
    const localOffset = buf.readUInt32LE(pos + 42);
    const name = buf.toString('utf8', pos + 46, pos + 46 + nameLen);
    pos += 46 + nameLen + extraLen + commentLen;
    if (name.endsWith('/')) continue;
    const localNameLen = buf.readUInt16LE(localOffset + 26);
    const localExtraLen = buf.readUInt16LE(localOffset + 28);
    const start = localOffset + 30 + localNameLen + localExtraLen;
    const body = buf.subarray(start, start + compressed);
    const data = method === 8 ? inflateRawSync(body) : Buffer.from(body);
    if (method !== 0 && method !== 8) throw new Error(`unsupported method ${method} for ${name}`);
    if (crc32(data) !== crc) throw new Error(`crc mismatch for ${name}`);
    out.push({ name, data, mode: madeBy === 3 ? (external >>> 16) & 0o7777 : null });
  }
  return out;
}
