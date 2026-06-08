import { describe, expect, it } from 'vitest';
// @ts-expect-error — plain ESM build script, no type declarations
import { buildLatestJson, assetUrl } from './make-latest-json.mjs';

describe('assetUrl', () => {
  it('builds a release download URL from repo, tag, and filename', () => {
    expect(assetUrl('kouji-dev/orrery-releases', 'v0.2.0', 'Orrery_0.2.0_x64-setup.exe')).toBe(
      'https://github.com/kouji-dev/orrery-releases/releases/download/v0.2.0/Orrery_0.2.0_x64-setup.exe',
    );
  });
});

describe('buildLatestJson', () => {
  it('shapes the Tauri v2 updater manifest for windows-x86_64', () => {
    const manifest = buildLatestJson({
      version: '0.2.0',
      notes: 'hi',
      pubDate: '2026-06-08T00:00:00.000Z',
      signature: 'SIG==',
      url: 'https://example/Orrery_0.2.0_x64-setup.exe',
    });
    expect(manifest).toEqual({
      version: '0.2.0',
      notes: 'hi',
      pub_date: '2026-06-08T00:00:00.000Z',
      platforms: {
        'windows-x86_64': { signature: 'SIG==', url: 'https://example/Orrery_0.2.0_x64-setup.exe' },
      },
    });
  });
});
