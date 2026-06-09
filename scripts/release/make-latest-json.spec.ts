import { describe, expect, it } from 'vitest';
// @ts-expect-error — plain ESM build script, no type declarations
import { buildLatestJson, buildPlatforms, assetUrl } from './make-latest-json.mjs';

describe('assetUrl', () => {
  it('builds a release download URL from repo, tag, and filename', () => {
    expect(assetUrl('kouji-dev/orrery-releases', 'v0.2.0', 'Orrery_0.2.0_x64-setup.exe')).toBe(
      'https://github.com/kouji-dev/orrery-releases/releases/download/v0.2.0/Orrery_0.2.0_x64-setup.exe',
    );
  });
});

describe('buildPlatforms', () => {
  it('maps NSIS to windows-x86_64 and MSI to windows-x86_64-msi', () => {
    const platforms = buildPlatforms('o/r', 'v0.2.0', [
      { key: 'windows-x86_64', file: 'Orrery_0.2.0_x64-setup.exe', signature: 'NSIS==' },
      { key: 'windows-x86_64-msi', file: 'Orrery_0.2.0_x64_en-US.msi', signature: 'MSI==' },
    ]);
    expect(platforms).toEqual({
      'windows-x86_64': {
        signature: 'NSIS==',
        url: 'https://github.com/o/r/releases/download/v0.2.0/Orrery_0.2.0_x64-setup.exe',
      },
      'windows-x86_64-msi': {
        signature: 'MSI==',
        url: 'https://github.com/o/r/releases/download/v0.2.0/Orrery_0.2.0_x64_en-US.msi',
      },
    });
  });
});

describe('buildLatestJson', () => {
  it('shapes the Tauri v2 updater manifest with the given platforms', () => {
    const manifest = buildLatestJson({
      version: '0.2.0',
      notes: 'hi',
      pubDate: '2026-06-08T00:00:00.000Z',
      platforms: {
        'windows-x86_64': { signature: 'SIG==', url: 'https://example/Orrery_0.2.0_x64-setup.exe' },
      },
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
