import { describe, expect, it } from 'vitest';
import { inflateRawSync } from 'node:zlib';
// @ts-expect-error — plain ESM build script, no type declarations
import { zipBuffer, readZipEntries } from './zip.mjs';

describe('zipBuffer', () => {
  it('round-trips deflated entries through node zlib', () => {
    const text = 'hello '.repeat(200);
    const buf = zipBuffer([
      { name: 'manifest.json', data: '{"a":1}' },
      { name: 'queries/tags.scm', data: text },
    ]);
    // local header of the first entry
    expect(buf.readUInt32LE(0)).toBe(0x04034b50);
    const entries = readZipEntries(buf);
    expect(entries.map((e: { name: string }) => e.name)).toEqual(['manifest.json', 'queries/tags.scm']);
    expect(entries[1].data.toString()).toBe(text);

    // second entry is deflated (method 8): inflate the raw body by hand
    const first = 30 + 'manifest.json'.length + buf.readUInt32LE(18);
    expect(buf.readUInt16LE(first + 8)).toBe(8);
    const nameLen = buf.readUInt16LE(first + 26);
    const size = buf.readUInt32LE(first + 18);
    const body = buf.subarray(first + 30 + nameLen, first + 30 + nameLen + size);
    expect(inflateRawSync(body).toString()).toBe(text);
  });

  it('stores incompressible data as method 0 and is deterministic', () => {
    const noise = Buffer.from(Array.from({ length: 64 }, (_, i) => (i * 97) % 256));
    const a = zipBuffer([{ name: 'bin', data: noise }]);
    const b = zipBuffer([{ name: 'bin', data: noise }]);
    expect(a.equals(b)).toBe(true);
    expect(a.readUInt16LE(8)).toBe(0);
    expect(readZipEntries(a)[0].data.equals(noise)).toBe(true);
  });

  it('stores a unix mode as external attributes and reads it back; entries without one read as null', () => {
    const buf = zipBuffer([
      { name: 'bin/node', data: 'ELF', mode: 0o755 },
      { name: 'LICENSE', data: 'MIT' },
    ]);
    const entries = readZipEntries(buf);
    expect(entries[0].mode).toBe(0o755);
    expect(entries[1].mode).toBeNull();
    // central directory of the first entry: version made by = unix (3), external attrs = S_IFREG|0755 << 16
    const cd = buf.readUInt32LE(buf.length - 22 + 16);
    expect(buf.readUInt16LE(cd + 4) >> 8).toBe(3);
    expect(buf.readUInt32LE(cd + 38) >>> 16).toBe(0o100755);
  });

  it('rejects a non-zip buffer', () => {
    expect(() => readZipEntries(Buffer.from('nope'))).toThrow(/not a zip/);
  });
});
