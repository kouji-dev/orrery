import { describe, expect, it } from 'vitest';
// @ts-expect-error — plain ESM build script, no type declarations
import { setJsonVersion, setCargoVersion } from './stamp-version.mjs';

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
