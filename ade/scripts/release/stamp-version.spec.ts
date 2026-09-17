import { describe, expect, it } from 'vitest';
// @ts-expect-error — plain ESM build script, no type declarations
import { setJsonVersion, setCargoVersion, setLockVersion } from './stamp-version.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { nextVersion } from './bump.mjs';

describe('setJsonVersion', () => {
  it('replaces only the first top-level version field', () => {
    const out = setJsonVersion('{\n  "name": "orrery",\n  "version": "0.1.0",\n  "deps": { "version": "9" }\n}', '0.2.0');
    expect(out).toContain('"version": "0.2.0"');
    expect(out).toContain('{ "version": "9" }'); // untouched
  });
});

describe('setCargoVersion', () => {
  it('replaces the package version, not dependency versions', () => {
    const cargo = '[package]\nname = "orrery"\nversion = "0.1.0"\n\n[dependencies]\ntauri = { version = "2" }\n';
    const out = setCargoVersion(cargo, '0.2.0');
    expect(out).toContain('version = "0.2.0"');
    expect(out).toContain('tauri = { version = "2" }'); // untouched
  });
});

describe('setLockVersion', () => {
  it("updates only the app's own lockfile entry (CRLF tolerated)", () => {
    const lock =
      '[[package]]\r\nname = "other"\r\nversion = "1.2.3"\r\n\r\n[[package]]\r\nname = "orrery"\r\nversion = "0.1.0"\r\n';
    const out = setLockVersion(lock, 'orrery', '0.2.0');
    expect(out).toContain('name = "orrery"\r\nversion = "0.2.0"');
    expect(out).toContain('name = "other"\r\nversion = "1.2.3"'); // untouched
  });
});

describe('buildNotes', () => {
  it('bullets subjects, dropping release-bump commits and blanks', async () => {
    // @ts-expect-error — plain ESM build script, no type declarations
    const { buildNotes } = await import('./notes.mjs');
    expect(buildNotes(['feat: a', 'release: v0.2.0', '', 'fix: b'])).toBe('- feat: a\n- fix: b');
  });
});

describe('nextVersion', () => {
  it('release bumps the minor and zeroes the patch', () => {
    expect(nextVersion('0.1.17', 'release')).toBe('0.2.0');
  });
  it('patch bumps the patch only', () => {
    expect(nextVersion('0.1.17', 'patch')).toBe('0.1.18');
  });
  it('rejects pre-release/garbage current versions and unknown kinds', () => {
    expect(() => nextVersion('0.2.0-beta.1', 'patch')).toThrow(/not plain semver/);
    expect(() => nextVersion('0.1.17', 'major')).toThrow(/unknown bump kind/);
  });
});
