import { describe, expect, it } from 'vitest';
import { gzipSync } from 'node:zlib';
import {
  RUNTIMES,
  nodeAssetFor,
  packEntries,
  runtimeManifest,
  runtimePackName,
  selectRuntimeFiles,
  temurinAssetFor,
  unpackArchive,
  // @ts-expect-error — plain ESM build script, no type declarations
} from './build-runtimes.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { loadServerPins } from './build-servers.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { parsePackName } from './build-grammar.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { readZipEntries, zipBuffer } from './zip.mjs';

const pins = loadServerPins();

describe('pins', () => {
  it('pin every runtime for every target with a sha256 and a binary path', () => {
    for (const runtime of RUNTIMES) {
      for (const target of ['windows-x86_64', 'macos-aarch64']) {
        const a = runtime === 'node' ? nodeAssetFor(target, pins) : temurinAssetFor(target, pins);
        expect(a.url).toMatch(/^https:\/\//);
        expect(a.sha256).toMatch(/^[0-9a-f]{64}$/);
        expect(a.unpack).toBe(target.startsWith('windows-') ? 'zip' : 'tar.gz');
        expect(a.binary).toBe(
          runtime === 'node'
            ? target.startsWith('windows-') ? 'node.exe' : 'bin/node'
            : target.startsWith('windows-') ? 'bin/java.exe' : 'Contents/Home/bin/java',
        );
      }
    }
  });

  it('nodeAssetFor pins Node 22 LTS from nodejs.org; temurinAssetFor pins a JRE 21 from adoptium', () => {
    const node = nodeAssetFor('windows-x86_64', pins);
    expect(node.version).toMatch(/^22\.\d+\.\d+$/);
    expect(node.url).toBe(`https://nodejs.org/dist/v${node.version}/node-v${node.version}-win-x64.zip`);
    const java = temurinAssetFor('macos-aarch64', pins);
    expect(java.version).toMatch(/^21\./);
    expect(java.release).toMatch(/^jdk-21/);
    expect(java.url).toContain('temurin21-binaries');
    expect(java.url).toContain('jre_aarch64_mac');
  });

  it('rejects unknown targets', () => {
    expect(() => nodeAssetFor('linux-x86_64', pins)).toThrow(/unknown target/);
  });
});

describe('runtimeManifest', () => {
  it('follows the pack contract for node', () => {
    const m = runtimeManifest('node', 'windows-x86_64', { pins, packRev: 2 });
    expect(m).toMatchObject({
      schema: 1,
      id: 'runtime.node',
      kind: 'runtime',
      version: `${pins.runtimes.node.version}-2`,
      runtime: 'node',
      binary: 'node.exe',
      target: 'windows-x86_64',
      minOrreryVersion: '0.24.0',
    });
    expect(m.label).toContain('Node');
    expect(m.upstream.sha256).toBe(pins.runtimes.node.assets['windows-x86_64'].sha256);
  });

  it('follows the pack contract for java and names a parseable zip', () => {
    const m = runtimeManifest('java', 'macos-aarch64', { pins });
    expect(m).toMatchObject({ id: 'runtime.java', kind: 'runtime', runtime: 'java', binary: 'Contents/Home/bin/java', target: 'macos-aarch64' });
    expect(m.upstream.release).toBe(pins.runtimes.java.release);
    const name = runtimePackName(m);
    expect(name).toBe(`runtime.java-${pins.runtimes.java.version}-1-macos-aarch64.zip`);
    expect(parsePackName(name)).toEqual({ id: 'runtime.java', version: `${pins.runtimes.java.version}-1`, target: 'macos-aarch64' });
  });

  it('rejects an unknown runtime', () => {
    expect(() => runtimeManifest('python', 'windows-x86_64', { pins })).toThrow(/unknown runtime/);
  });
});

// ustar entry (see tar.spec.ts for the full writer)
function tarEntry(name: string, data: string, mode = 0o644) {
  const h = Buffer.alloc(512);
  h.write(name, 0, 100);
  h.write(mode.toString(8).padStart(7, '0'), 100, 8);
  h.write('0000000', 108, 8);
  h.write('0000000', 116, 8);
  h.write(data.length.toString(8).padStart(11, '0'), 124, 12);
  h.write('00000000000', 136, 12);
  h.write('        ', 148, 8);
  h.write('0', 156, 1);
  h.write('ustar\0', 257, 6);
  h.write('00', 263, 2);
  let sum = 0;
  for (const b of h) sum += b;
  h.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8);
  const body = Buffer.alloc(Math.ceil(data.length / 512) * 512);
  body.write(data);
  return Buffer.concat([h, body]);
}

describe('unpackArchive + selectRuntimeFiles', () => {
  it('strips the top-level directory of a zip and keeps only node + LICENSE for node', () => {
    const zip = zipBuffer([
      { name: 'node-v22.0.0-win-x64/node.exe', data: 'MZ' },
      { name: 'node-v22.0.0-win-x64/LICENSE', data: 'MIT' },
      { name: 'node-v22.0.0-win-x64/npm.cmd', data: 'x' },
    ]);
    const all = unpackArchive(zip, 'zip');
    expect(all.map((e: { name: string }) => e.name)).toEqual(['node.exe', 'LICENSE', 'npm.cmd']);
    const kept = selectRuntimeFiles('node', all, 'node.exe');
    expect(kept.map((e: { name: string }) => e.name).sort()).toEqual(['LICENSE', 'node.exe']);
  });

  it('keeps the whole tree for java (tar.gz, modes preserved) and rejects a missing binary', () => {
    const tgz = gzipSync(Buffer.concat([tarEntry('jdk-21-jre/bin/java', 'ELF', 0o755), tarEntry('jdk-21-jre/lib/modules', 'JIMAGE'), Buffer.alloc(1024)]));
    const all = unpackArchive(tgz, 'tar.gz');
    expect(selectRuntimeFiles('java', all, 'bin/java')).toHaveLength(2);
    expect(all.find((e: { name: string }) => e.name === 'bin/java').mode).toBe(0o755);
    expect(() => selectRuntimeFiles('java', all, 'Contents/Home/bin/java')).toThrow(/no Contents\/Home\/bin\/java/);
    expect(() => unpackArchive(tgz, 'rar')).toThrow(/unsupported unpack/);
  });
});

describe('packEntries', () => {
  it('puts manifest.json first, sorts files, and carries executable bits into the zip', () => {
    const entries = packEntries({ id: 'runtime.node' }, [
      { name: 'bin/node', data: Buffer.from('x'), mode: 0o755 },
      { name: 'LICENSE', data: Buffer.from('y'), mode: 0o644 },
    ]);
    expect(entries.map((e: { name: string }) => e.name)).toEqual(['manifest.json', 'LICENSE', 'bin/node']);
    const back = readZipEntries(zipBuffer(entries));
    expect(back.find((e: { name: string }) => e.name === 'bin/node').mode).toBe(0o755);
    expect(back.find((e: { name: string }) => e.name === 'manifest.json').mode).toBeNull();
  });
});
