import { describe, expect, it } from 'vitest';
import { sep } from 'node:path';
import {
  buildManifest,
  compileArgs,
  composeQuery,
  libraryName,
  loadPins,
  packName,
  packRevOf,
  packVersion,
  parseAbi,
  parseArgs,
  parsePackName,
  selectPins,
  tarballUrl,
  // @ts-expect-error — plain ESM build script, no type declarations
} from './build-grammar.mjs';

const java = {
  id: 'grammar.java',
  repo: 'tree-sitter/tree-sitter-java',
  tag: 'v0.23.5',
  sha256: '',
  symbol: 'tree_sitter_java',
  language: 'java',
  monacoLanguage: 'java',
  fileExtensions: ['java'],
  hasLocals: false,
};

describe('buildManifest', () => {
  it('shapes the grammar manifest per the registry schema', () => {
    expect(buildManifest(java, { target: 'windows-x86_64', packRev: 1, abi: 14 })).toEqual({
      schema: 1,
      id: 'grammar.java',
      kind: 'grammar',
      version: '0.23.5-1',
      label: 'Java grammar',
      language: 'java',
      monacoLanguage: 'java',
      fileExtensions: ['java'],
      symbol: 'tree_sitter_java',
      abi: 14,
      library: 'tree_sitter_java.dll',
      queries: { tags: 'queries/tags.scm', locals: null },
      upstream: { repo: 'tree-sitter/tree-sitter-java', tag: 'v0.23.5' },
      target: 'windows-x86_64',
      minOrreryVersion: '0.24.0',
    });
  });

  it('bumps the pack revision, picks the dylib on macOS, and lists locals when pinned', () => {
    const ts = { ...java, id: 'grammar.typescript', language: 'typescript', symbol: 'tree_sitter_typescript', hasLocals: true, tag: 'v0.23.2' };
    const m = buildManifest(ts, { target: 'macos-aarch64', packRev: 3, abi: 14 });
    expect(m.version).toBe('0.23.2-3');
    expect(m.library).toBe('tree_sitter_typescript.dylib');
    expect(m.label).toBe('TypeScript grammar');
    expect(m.queries).toEqual({ tags: 'queries/tags.scm', locals: 'queries/locals.scm' });
  });

  it('rejects an unknown target', () => {
    expect(() => buildManifest(java, { target: 'linux-x86_64' })).toThrow(/unknown target/);
  });
});

describe('naming', () => {
  it('packVersion strips the v prefix and appends the revision', () => {
    expect(packVersion('v0.23.5')).toBe('0.23.5-1');
    expect(packVersion('v0.23.5', 2)).toBe('0.23.5-2');
  });

  it('libraryName follows the target platform', () => {
    expect(libraryName('go', 'windows-x86_64')).toBe('tree_sitter_go.dll');
    expect(libraryName('go', 'macos-aarch64')).toBe('tree_sitter_go.dylib');
  });

  it('packName / parsePackName round-trip ids and targets containing dashes', () => {
    const name = packName('server.typescript-language-server', '6.0.0-1', 'any');
    expect(name).toBe('server.typescript-language-server-6.0.0-1-any.zip');
    expect(parsePackName(name)).toEqual({ id: 'server.typescript-language-server', version: '6.0.0-1', target: 'any' });
    expect(parsePackName('grammar.java-0.23.5-1-windows-x86_64.zip')).toEqual({
      id: 'grammar.java',
      version: '0.23.5-1',
      target: 'windows-x86_64',
    });
    expect(parsePackName('runtime.java-21.0.12.1-1-macos-aarch64.zip')).toEqual({ id: 'runtime.java', version: '21.0.12.1-1', target: 'macos-aarch64' });
    expect(parsePackName('index.json')).toBeNull();
  });

  it('tarballUrl points at the GitHub tag archive', () => {
    expect(tarballUrl(java)).toBe('https://github.com/tree-sitter/tree-sitter-java/archive/refs/tags/v0.23.5.tar.gz');
  });
});

describe('compileArgs', () => {
  it('uses cl.exe /LD on Windows', () => {
    const { cmd, args } = compileArgs('windows-x86_64', { srcDir: 'S', hasScanner: true, out: 'O/tree_sitter_java.dll' });
    expect(cmd).toBe('cl');
    expect(args).toEqual(['/LD', '/O2', '/IS', `S${sep}parser.c`, `S${sep}scanner.c`, '/Fe:O/tree_sitter_java.dll']);
  });

  it('uses clang -shared on macOS and links libc++ for a C++ scanner', () => {
    const plain = compileArgs('macos-aarch64', { srcDir: 'S', out: 'lib.dylib' });
    expect(plain.cmd).toBe('clang');
    expect(plain.args).toEqual(['-shared', '-O2', '-fPIC', '-IS', `S${sep}parser.c`, '-o', 'lib.dylib']);

    const cc = compileArgs('macos-aarch64', { srcDir: 'S', hasScanner: true, hasScannerCc: true, out: 'lib.dylib' });
    expect(cc.args).toContain(`S${sep}scanner.cc`);
    expect(cc.args).not.toContain(`S${sep}scanner.c`);
    expect(cc.args.at(-1)).toBe('-lc++');
  });
});

describe('parseAbi', () => {
  it('reads LANGUAGE_VERSION from parser.c, falling back to parser.h', () => {
    expect(parseAbi('#include "tree_sitter/parser.h"\n\n#define LANGUAGE_VERSION 15\n#define STATE_COUNT 10')).toBe(15);
    expect(parseAbi('', '#define TREE_SITTER_LANGUAGE_VERSION 14')).toBe(14);
    expect(parseAbi('', '')).toBeNull();
  });
});

describe('composeQuery', () => {
  it('concatenates the parts in order, each under a label line, one blank line apart', () => {
    const out = composeQuery([
      { label: 'grammar.typescript tags.scm (tree-sitter/tree-sitter-typescript v0.23.2)', text: '(interface_declaration\n  name: (type_identifier) @name) @definition.interface\n\n\n' },
      { label: 'inherited from grammar.javascript tags.scm (tree-sitter/tree-sitter-javascript v0.25.0)', text: '(class_declaration\n  name: (_) @name) @definition.class' },
    ]);
    expect(out).toBe(
      '; ---- grammar.typescript tags.scm (tree-sitter/tree-sitter-typescript v0.23.2) ----\n' +
        '(interface_declaration\n  name: (type_identifier) @name) @definition.interface\n' +
        '\n' +
        '; ---- inherited from grammar.javascript tags.scm (tree-sitter/tree-sitter-javascript v0.25.0) ----\n' +
        '(class_declaration\n  name: (_) @name) @definition.class\n',
    );
  });
});

describe('packRevOf', () => {
  it('prefers the CLI --rev, then the pin, then 1', () => {
    expect(packRevOf({ packRev: 2 }, '3')).toBe(3);
    expect(packRevOf({ packRev: 2 }, undefined)).toBe(2);
    expect(packRevOf({ packRev: 2 }, '')).toBe(2); // the workflow's blank default
    expect(packRevOf({ packRev: 2 }, true)).toBe(2); // a bare --rev flag
    expect(packRevOf({}, undefined)).toBe(1);
  });
});

describe('parseArgs', () => {
  it('reads pairs and bare flags', () => {
    expect(parseArgs(['--lang', 'java', '--local', '--out', 'x'])).toEqual({ lang: 'java', local: true, out: 'x' });
    expect(parseArgs(['--local'])).toEqual({ local: true });
  });
});

describe('grammars.json pins', () => {
  const pins = loadPins();

  it('covers the seven v1 grammars with canonical ids and a v-tag each', () => {
    expect(pins.map((p: { id: string }) => p.id)).toEqual([
      'grammar.java',
      'grammar.typescript',
      'grammar.tsx',
      'grammar.javascript',
      'grammar.rust',
      'grammar.python',
      'grammar.go',
    ]);
    for (const p of pins) {
      expect(p.tag).toMatch(/^v\d+\.\d+\.\d+$/);
      expect(p.symbol).toBe(`tree_sitter_${p.language}`);
      expect(p.fileExtensions.length).toBeGreaterThan(0);
    }
  });

  it('typescript and tsx compose their queries with javascript (upstream tree-sitter.json lists both files) at pack rev 2', () => {
    // rev 2 because the shipped tags.scm changed under the same upstream tag —
    // the app's parsed-symbol cache is keyed by pack version
    for (const lang of ['typescript', 'tsx']) {
      const pin = selectPins(pins, lang)[0];
      expect(pin.inherits).toBe('javascript');
      expect(pins.some((p: { language: string }) => p.language === pin.inherits)).toBe(true);
      expect(packRevOf(pin, undefined)).toBe(2);
    }
    for (const p of pins) if (!p.inherits) expect(packRevOf(p, undefined)).toBe(1);
  });

  it('selectPins resolves by language, id, csv, and all', () => {
    expect(selectPins(pins, 'java')[0].id).toBe('grammar.java');
    expect(selectPins(pins, 'grammar.tsx')[0].subdir).toBe('tsx');
    expect(selectPins(pins, 'rust,go').map((p: { id: string }) => p.id)).toEqual(['grammar.rust', 'grammar.go']);
    expect(selectPins(pins, 'all')).toHaveLength(7);
    expect(() => selectPins(pins, 'cobol')).toThrow(/no grammar pin/);
  });
});
