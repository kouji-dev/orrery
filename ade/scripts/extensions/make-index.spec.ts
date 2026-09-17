import { describe, expect, it } from 'vitest';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
// @ts-expect-error — plain ESM build script, no type declarations
import { artifactUrl, buildIndex, packEntry, readArtifacts, releasePlan, releaseTag } from './make-index.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { zipBuffer } from './zip.mjs';

const grammar = (target: string, version = '0.23.5-1') => ({
  schema: 1,
  id: 'grammar.java',
  kind: 'grammar',
  version,
  label: 'Java grammar',
  language: 'java',
  abi: 14,
  target,
  minOrreryVersion: '0.24.0',
});

const art = (manifest: Record<string, unknown>, file: string) => ({
  file,
  path: join('dist-ext', file),
  manifest,
  sha256: `sha-${file}`,
  size: file.length,
});

describe('artifactUrl', () => {
  it('points at the per-pack release in orrery-releases', () => {
    expect(releaseTag('grammar.java', '0.23.5-1')).toBe('ext-grammar.java-v0.23.5-1');
    expect(artifactUrl('kouji-dev/orrery-releases', 'grammar.java', '0.23.5-1', 'grammar.java-0.23.5-1-windows-x86_64.zip')).toBe(
      'https://github.com/kouji-dev/orrery-releases/releases/download/ext-grammar.java-v0.23.5-1/grammar.java-0.23.5-1-windows-x86_64.zip',
    );
  });
});

describe('packEntry', () => {
  it('keeps abi for grammars and the language list for servers', () => {
    expect(packEntry(grammar('windows-x86_64'))).toEqual({
      id: 'grammar.java',
      kind: 'grammar',
      version: '0.23.5-1',
      description: 'Java grammar',
      languages: ['java'],
      abi: 14,
      minOrreryVersion: '0.24.0',
    });
    expect(packEntry({ id: 'server.jdtls', kind: 'server', version: '1.61.0-1', label: 'Eclipse JDT LS', languages: ['java'], minOrreryVersion: '0.24.0' })).toEqual({
      id: 'server.jdtls',
      kind: 'server',
      version: '1.61.0-1',
      description: 'Eclipse JDT LS',
      languages: ['java'],
      minOrreryVersion: '0.24.0',
    });
  });
});

describe('packEntry (runtime + requires)', () => {
  it('mirrors requires from server manifests and runtime from runtime manifests', () => {
    const server = { id: 'server.pyright', kind: 'server', version: '1.1.414-1', label: 'Pyright', languages: ['python'], target: 'any', requires: ['runtime.node'], launch: { runtime: 'node', entry: 'x', args: [] }, minOrreryVersion: '0.24.0' };
    expect(packEntry(server)).toEqual({
      id: 'server.pyright',
      kind: 'server',
      version: '1.1.414-1',
      description: 'Pyright',
      languages: ['python'],
      requires: ['runtime.node'],
      minOrreryVersion: '0.24.0',
    });
    expect(packEntry({ id: 'runtime.node', kind: 'runtime', version: '22.23.2-1', label: 'Node.js 22 runtime', runtime: 'node', binary: 'node.exe', target: 'windows-x86_64', minOrreryVersion: '0.24.0' })).toEqual({
      id: 'runtime.node',
      kind: 'runtime',
      version: '22.23.2-1',
      description: 'Node.js 22 runtime',
      languages: [],
      runtime: 'node',
      minOrreryVersion: '0.24.0',
    });
  });
});

describe('buildIndex', () => {
  const opts = { repo: 'o/r', generated: '2026-09-11T00:00:00.000Z' };

  it('files runtime packs and per-target server packs under their manifest target, servers without one under any', () => {
    const idx = buildIndex(null, [
      art({ id: 'runtime.node', kind: 'runtime', version: '22.23.2-1', label: 'Node', runtime: 'node', target: 'windows-x86_64', minOrreryVersion: '0.24.0' }, 'runtime.node-22.23.2-1-windows-x86_64.zip'),
      art({ id: 'runtime.node', kind: 'runtime', version: '22.23.2-1', label: 'Node', runtime: 'node', target: 'macos-aarch64', minOrreryVersion: '0.24.0' }, 'runtime.node-22.23.2-1-macos-aarch64.zip'),
      art({ id: 'server.gopls', kind: 'server', version: '0.19.1-1', label: 'gopls', languages: ['go'], target: 'windows-x86_64', requires: [], minOrreryVersion: '0.24.0' }, 'server.gopls-0.19.1-1-windows-x86_64.zip'),
      art({ id: 'server.jdtls', kind: 'server', version: '1.61.0-1', label: 'jdtls', languages: ['java'], target: 'any', requires: ['runtime.java'], minOrreryVersion: '0.24.0' }, 'server.jdtls-1.61.0-1-any.zip'),
    ], opts);
    const byId = Object.fromEntries(idx.packs.map((p: { id: string }) => [p.id, p]));
    expect(Object.keys(byId['runtime.node'].targets).sort()).toEqual(['macos-aarch64', 'windows-x86_64']);
    expect(Object.keys(byId['server.gopls'].targets)).toEqual(['windows-x86_64']);
    expect(Object.keys(byId['server.jdtls'].targets)).toEqual(['any']);
    expect(byId['server.jdtls'].requires).toEqual(['runtime.java']);
    expect(byId['server.jdtls'].targets.any.sizeBytes).toBe('server.jdtls-1.61.0-1-any.zip'.length);
    expect(byId['runtime.node'].requires).toBeUndefined();
  });

  it('builds a fresh index with one target per artifact, sorted by id', () => {
    const idx = buildIndex(null, [
      art({ id: 'server.gopls', kind: 'server', version: '0.23.0-1', label: 'gopls', languages: ['go'], minOrreryVersion: '0.24.0' }, 'server.gopls-0.23.0-1-any.zip'),
      art(grammar('windows-x86_64'), 'grammar.java-0.23.5-1-windows-x86_64.zip'),
      art(grammar('macos-aarch64'), 'grammar.java-0.23.5-1-macos-aarch64.zip'),
    ], opts);
    expect(idx.schema).toBe(1);
    expect(idx.generated).toBe(opts.generated);
    expect(idx.packs.map((p: { id: string }) => p.id)).toEqual(['grammar.java', 'server.gopls']);
    expect(idx.packs[0].targets).toEqual({
      'windows-x86_64': {
        url: 'https://github.com/o/r/releases/download/ext-grammar.java-v0.23.5-1/grammar.java-0.23.5-1-windows-x86_64.zip',
        sha256: 'sha-grammar.java-0.23.5-1-windows-x86_64.zip',
        size: 'grammar.java-0.23.5-1-windows-x86_64.zip'.length,
        sizeBytes: 'grammar.java-0.23.5-1-windows-x86_64.zip'.length,
      },
      'macos-aarch64': expect.objectContaining({ sha256: 'sha-grammar.java-0.23.5-1-macos-aarch64.zip' }),
    });
    expect(Object.keys(idx.packs[1].targets)).toEqual(['any']);
  });

  it('keeps untouched packs, replaces a rebuilt id at a new version, merges targets at the same version', () => {
    const prev = buildIndex(null, [
      art(grammar('windows-x86_64'), 'grammar.java-0.23.5-1-windows-x86_64.zip'),
      art(grammar('macos-aarch64'), 'grammar.java-0.23.5-1-macos-aarch64.zip'),
      art({ id: 'grammar.go', kind: 'grammar', version: '0.25.0-1', label: 'Go grammar', language: 'go', abi: 15, target: 'windows-x86_64', minOrreryVersion: '0.24.0' }, 'grammar.go-0.25.0-1-windows-x86_64.zip'),
    ], opts);

    // new version of java, windows only → mac entry of the old version is dropped
    const next = buildIndex(prev, [art(grammar('windows-x86_64', '0.23.5-2'), 'grammar.java-0.23.5-2-windows-x86_64.zip')], opts);
    const java = next.packs.find((p: { id: string }) => p.id === 'grammar.java');
    expect(java.version).toBe('0.23.5-2');
    expect(Object.keys(java.targets)).toEqual(['windows-x86_64']);
    // go untouched
    expect(next.packs.find((p: { id: string }) => p.id === 'grammar.go')).toEqual(prev.packs.find((p: { id: string }) => p.id === 'grammar.go'));

    // same version, other platform → both targets present
    const both = buildIndex(next, [art(grammar('macos-aarch64', '0.23.5-2'), 'grammar.java-0.23.5-2-macos-aarch64.zip')], opts);
    expect(Object.keys(both.packs.find((p: { id: string }) => p.id === 'grammar.java').targets).sort()).toEqual(['macos-aarch64', 'windows-x86_64']);
    // prev is not mutated
    expect(Object.keys(prev.packs.find((p: { id: string }) => p.id === 'grammar.java').targets)).toHaveLength(2);
  });

  it('writes file:/// URLs in local mode', () => {
    const a = art(grammar('windows-x86_64'), 'grammar.java-0.23.5-1-windows-x86_64.zip');
    const idx = buildIndex(null, [a], { ...opts, local: true });
    const url = idx.packs[0].targets['windows-x86_64'].url;
    expect(url.startsWith('file:///')).toBe(true);
    expect(url).toBe(pathToFileURL(join(process.cwd(), a.path)).href);
  });
});

describe('readArtifacts + releasePlan', () => {
  it('reads manifest, sha256 and size out of pack zips and groups them per release tag', () => {
    const dir = mkdtempSync(join(tmpdir(), 'orrery-ext-'));
    const win = zipBuffer([{ name: 'manifest.json', data: JSON.stringify(grammar('windows-x86_64')) }]);
    const mac = zipBuffer([{ name: 'manifest.json', data: JSON.stringify(grammar('macos-aarch64')) }]);
    writeFileSync(join(dir, 'grammar.java-0.23.5-1-windows-x86_64.zip'), win);
    writeFileSync(join(dir, 'grammar.java-0.23.5-1-macos-aarch64.zip'), mac);
    writeFileSync(join(dir, 'index.json'), '{}');
    writeFileSync(join(dir, 'notes.zip'), zipBuffer([{ name: 'x', data: 'y' }]));

    const arts = readArtifacts(dir);
    expect(arts.map((a: { file: string }) => a.file)).toEqual([
      'grammar.java-0.23.5-1-macos-aarch64.zip',
      'grammar.java-0.23.5-1-windows-x86_64.zip',
    ]);
    expect(arts[1].manifest.target).toBe('windows-x86_64');
    expect(arts[1].size).toBe(win.length);
    expect(arts[1].sha256).toMatch(/^[0-9a-f]{64}$/);

    expect(releasePlan(arts)).toEqual([
      {
        tag: 'ext-grammar.java-v0.23.5-1',
        id: 'grammar.java',
        version: '0.23.5-1',
        files: ['grammar.java-0.23.5-1-macos-aarch64.zip', 'grammar.java-0.23.5-1-windows-x86_64.zip'],
      },
    ]);
  });
});
