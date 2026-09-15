import { describe, expect, it } from 'vitest';
import { gzipSync } from 'node:zlib';
// @ts-expect-error — plain ESM build script, no type declarations
import { dereference, parseNumeric, parsePax, readTar, readTarGz, stripComponents } from './tar.mjs';

// Hand-rolled ustar writer so the spec does not depend on a tar binary.
function header(fields: { name: string; size: number; type: string; mode?: number; linkname?: string; prefix?: string }) {
  const h = Buffer.alloc(512);
  h.write(fields.name, 0, 100, 'utf8');
  h.write((fields.mode ?? 0o644).toString(8).padStart(7, '0'), 100, 8, 'utf8');
  h.write('0000000', 108, 8, 'utf8');
  h.write('0000000', 116, 8, 'utf8');
  h.write(fields.size.toString(8).padStart(11, '0'), 124, 12, 'utf8');
  h.write('00000000000', 136, 12, 'utf8');
  h.write('        ', 148, 8, 'utf8');
  h.write(fields.type, 156, 1, 'utf8');
  if (fields.linkname) h.write(fields.linkname, 157, 100, 'utf8');
  h.write('ustar\0', 257, 6, 'utf8');
  h.write('00', 263, 2, 'utf8');
  if (fields.prefix) h.write(fields.prefix, 345, 155, 'utf8');
  let sum = 0;
  for (const b of h) sum += b;
  h.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'utf8');
  return h;
}

function entry(name: string, data: string | Buffer, opts: { type?: string; mode?: number; linkname?: string; prefix?: string } = {}) {
  const buf = Buffer.isBuffer(data) ? data : Buffer.from(data);
  const padded = Buffer.alloc(Math.ceil(buf.length / 512) * 512);
  buf.copy(padded);
  return Buffer.concat([header({ name, size: opts.type === '2' || opts.type === '5' ? 0 : buf.length, type: opts.type ?? '0', ...opts }), opts.type === '2' || opts.type === '5' ? Buffer.alloc(0) : padded]);
}

/** One pax record: `<len> <key>=<value>\n` where len counts itself. */
function pax(key: string, value: string) {
  const body = ` ${key}=${value}\n`;
  let len = body.length + 1;
  while (String(len).length + body.length !== len) len++;
  return `${len}${body}`;
}

function tar(...parts: Buffer[]) {
  return Buffer.concat([...parts, Buffer.alloc(1024)]);
}

describe('readTar', () => {
  it('reads files, modes, dirs (skipped) and ustar prefixes', () => {
    const buf = tar(
      entry('top/', '', { type: '5' }),
      entry('top/a.txt', 'hello'),
      entry('bin/tool', Buffer.from([1, 2, 3]), { mode: 0o755, prefix: 'top' }),
    );
    const entries = readTar(buf);
    expect(entries.map((e: { name: string }) => e.name)).toEqual(['top/a.txt', 'top/bin/tool']);
    expect(entries[0].data.toString()).toBe('hello');
    expect(entries[1].mode).toBe(0o755);
    expect(entries[1].type).toBe('file');
  });

  it('honours GNU long names and pax path/linkpath records', () => {
    const long = 'x'.repeat(150) + '/file.txt';
    const link = 'y'.repeat(120) + '/link';
    const buf = tar(
      entry('././@LongLink', long + '\0', { type: 'L' }),
      entry('truncated', 'data'),
      entry('PaxHeader/link', pax('path', link) + pax('linkpath', 'file.txt'), { type: 'x' }),
      entry('ignored', '', { type: '2', linkname: 'nope' }),
    );
    const entries = readTar(buf);
    expect(entries[0].name).toBe(long);
    expect(entries[0].data.toString()).toBe('data');
    expect(entries[1]).toMatchObject({ name: link, type: 'symlink', linkname: 'file.txt' });
  });

  it('readTarGz gunzips first', () => {
    const gz = gzipSync(tar(entry('a', 'b')));
    expect(readTarGz(gz)[0].name).toBe('a');
  });
});

describe('parseNumeric + parsePax', () => {
  it('parses octal text and base-256 sizes', () => {
    expect(parseNumeric(Buffer.from('00000000144\0'), 0, 12)).toBe(100);
    const big = Buffer.alloc(12);
    big[0] = 0x80;
    big.writeUInt32BE(5_000_000_000 >>> 0, 8);
    big[7] = 1; // 2^32 + (5e9 mod 2^32)
    expect(parseNumeric(big, 0, 12)).toBe(5_000_000_000);
  });

  it('parses pax records', () => {
    expect(parsePax(Buffer.from('12 path=a/b\n17 linkpath=../c\n'))).toEqual({ path: 'a/b', linkpath: '../c' });
    expect(pax('path', 'a/b')).toBe('12 path=a/b\n');
  });
});

describe('dereference + stripComponents', () => {
  it('copies the symlink target bytes under the link name, following chains, and drops dangling links', () => {
    const entries = readTar(
      tar(
        entry('d/lib/real.dylib', 'BYTES', { mode: 0o755 }),
        entry('d/MacOS/lib.dylib', '', { type: '2', linkname: '../lib/real.dylib' }),
        entry('d/alias', '', { type: '2', linkname: 'MacOS/lib.dylib' }),
        entry('d/dangling', '', { type: '2', linkname: 'missing' }),
      ),
    );
    const files = dereference(entries);
    expect(files.map((f: { name: string }) => f.name).sort()).toEqual(['d/MacOS/lib.dylib', 'd/alias', 'd/lib/real.dylib']);
    const alias = files.find((f: { name: string }) => f.name === 'd/alias');
    expect(alias.data.toString()).toBe('BYTES');
    expect(alias.mode).toBe(0o755);
    expect(alias.linkname).toBeNull();
  });

  it('strips leading path components and drops entries that run out', () => {
    const out = stripComponents([{ name: 'top/bin/node' }, { name: 'top' }, { name: 'top/LICENSE' }], 1);
    expect(out.map((e: { name: string }) => e.name)).toEqual(['bin/node', 'LICENSE']);
    expect(stripComponents([{ name: 'a/b' }], 0)).toEqual([{ name: 'a/b' }]);
  });
});
