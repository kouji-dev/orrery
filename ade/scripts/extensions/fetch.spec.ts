import { describe, expect, it, vi } from 'vitest';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
// @ts-expect-error — plain ESM build script, no type declarations
import { cacheName, fetchAsset, verify } from './fetch.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { sha256 } from './build-grammar.mjs';

describe('cacheName', () => {
  it('is the percent-decoded last path segment', () => {
    expect(cacheName('https://nodejs.org/dist/v22.23.2/node-v22.23.2-win-x64.zip')).toBe('node-v22.23.2-win-x64.zip');
    expect(cacheName('https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip')).toBe(
      'OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip',
    );
  });
});

describe('verify', () => {
  it('accepts a matching pin, throws on mismatch, warns when unpinned', () => {
    const buf = Buffer.from('payload');
    expect(verify(buf, sha256(buf), 'x')).toBe(sha256(buf));
    expect(() => verify(buf, 'deadbeef', 'x')).toThrow(/sha256 mismatch for x/);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    verify(buf, '', 'x');
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no sha256 pinned'));
    warn.mockRestore();
  });
});

describe('fetchAsset', () => {
  it('uses a verified cache entry without touching the network', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'orrery-fetch-'));
    const buf = Buffer.from('cached bytes');
    writeFileSync(join(dir, 'a.zip'), buf);
    const spy = vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('offline'));
    vi.spyOn(console, 'log').mockImplementation(() => {});
    const out = await fetchAsset({ url: 'https://example.invalid/x/a.zip', sha256: sha256(buf) }, dir);
    expect(out.equals(buf)).toBe(true);
    expect(spy).not.toHaveBeenCalled();
    vi.restoreAllMocks();
  });

  it('re-downloads a corrupt cache entry and rejects a bad upstream digest', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'orrery-fetch-'));
    const good = Buffer.from('fresh bytes');
    writeFileSync(join(dir, 'a.zip'), 'garbage');
    vi.spyOn(console, 'log').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const spy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(good));
    const out = await fetchAsset({ url: 'https://example.invalid/x/a.zip', sha256: sha256(good) }, dir);
    expect(out.equals(good)).toBe(true);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(readFileSync(join(dir, 'a.zip')).equals(good)).toBe(true);

    spy.mockResolvedValue(new Response(Buffer.from('tampered')));
    await expect(fetchAsset({ url: 'https://example.invalid/x/b.zip', sha256: sha256(good) }, dir)).rejects.toThrow(/sha256 mismatch/);
    expect(existsSync(join(dir, 'b.zip'))).toBe(false);
    vi.restoreAllMocks();
  });
});
