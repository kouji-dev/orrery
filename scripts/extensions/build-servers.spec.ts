import { describe, expect, it } from 'vitest';
import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  goInstallArgs,
  hostTarget,
  isTargetSpecific,
  keepNodeFile,
  loadServerManifests,
  loadServerPins,
  npmInstallArgs,
  packEntries,
  selectServers,
  serverManifest,
  serverPackName,
  walkFiles,
  // @ts-expect-error — plain ESM build script, no type declarations
} from './build-servers.mjs';
// @ts-expect-error — plain ESM build script, no type declarations
import { parsePackName } from './build-grammar.mjs';

const pins = loadServerPins();
const templates = loadServerManifests();
const byId = (id: string) => templates.find((t: { id: string }) => t.id === id);

describe('templates + pins (contract)', () => {
  it('every template has a pin, a launch recipe and a requires list', () => {
    expect(templates.map((t: { id: string }) => t.id).sort()).toEqual([
      'server.gopls',
      'server.jdtls',
      'server.pyright',
      'server.rust-analyzer',
      'server.typescript-language-server',
    ]);
    for (const t of templates) {
      expect(pins.servers[t.id], t.id).toBeDefined();
      expect(t.schema).toBe(1);
      expect(t.kind).toBe('server');
      expect(t.transport).toBe('stdio');
      expect(Array.isArray(t.requires)).toBe(true);
      expect(typeof t.launch.entry).toBe('string');
      expect(Array.isArray(t.launch.args)).toBe(true);
      expect(t.detect.path.length).toBeGreaterThan(0);
      expect(t.minOrreryVersion).toBe('0.24.0');
      expect(['any', 'per-target']).toContain(t.target);
    }
  });

  it('node servers require runtime.node and launch through it', () => {
    const ts = byId('server.typescript-language-server');
    expect(ts.requires).toEqual(['runtime.node']);
    expect(ts.launch).toEqual({ runtime: 'node', entry: 'server/node_modules/typescript-language-server/lib/cli.mjs', args: ['--stdio'] });
    expect(ts.initializationOptions.tsserver).toEqual({ useSyntaxServer: 'never', path: '${extDir}/server/node_modules/typescript/lib/tsserver.js' });
    expect(pins.servers[ts.id].npm).toEqual({ 'typescript-language-server': '6.0.0', typescript: '6.0.3' });
    const py = byId('server.pyright');
    expect(py.requires).toEqual(['runtime.node']);
    expect(py.launch).toEqual({ runtime: 'node', entry: 'server/node_modules/pyright/langserver.index.js', args: ['--stdio'] });
    expect(pins.servers[py.id].npm).toEqual({ pyright: '1.1.414' });
  });

  it('jdtls requires runtime.java, launches the equinox jar directly and documents ${jdtlsConfig}', () => {
    const j = byId('server.jdtls');
    expect(j.target).toBe('any');
    expect(j.requires).toEqual(['runtime.java']);
    expect(j.launch.runtime).toBe('java');
    expect(j.launch.entry).toBe('plugins/org.eclipse.equinox.launcher_*.jar');
    expect(j.launch.jvmArgs).toContain('-Declipse.application=org.eclipse.jdt.ls.core.id1');
    expect(j.launch.jvmArgs).toContain('--add-modules=ALL-SYSTEM');
    expect(j.launch.args).toEqual(['-configuration', '${extDir}/${jdtlsConfig}', '-data', '${workspaceStorage}']);
    expect(j.placeholders.jdtlsConfig).toEqual({ 'windows-x86_64': 'config_win', 'macos-aarch64': 'config_mac_arm', 'linux-x86_64': 'config_linux' });
    expect(j.virtualSchemes).toEqual(['jdt']);
    expect(j.virtualRead).toEqual({ method: 'java/classFileContents' });
    expect(j.initializationOptions.extendedClientCapabilities.classFileContentsSupport).toBe(true);
    expect(pins.servers[j.id].asset.sha256).toMatch(/^[0-9a-f]{64}$/);
  });

  it('native servers are per-target with no runtime; rust-analyzer assets are pinned per target', () => {
    expect(isTargetSpecific(byId('server.gopls'))).toBe(true);
    expect(isTargetSpecific(byId('server.rust-analyzer'))).toBe(true);
    expect(isTargetSpecific(byId('server.jdtls'))).toBe(false);
    expect(byId('server.gopls').launch).toEqual({ runtime: null, entry: 'gopls', args: [] });
    expect(pins.servers['server.gopls'].go).toEqual({ module: 'golang.org/x/tools/gopls', version: 'v0.19.1' });
    for (const target of ['windows-x86_64', 'macos-aarch64']) {
      const a = pins.servers['server.rust-analyzer'].assets[target];
      expect(a.url).toContain(`/releases/download/${pins.servers['server.rust-analyzer'].tag}/`);
      expect(a.sha256).toMatch(/^[0-9a-f]{64}$/);
    }
  });
});

describe('serverManifest', () => {
  it('stamps version from the pin, fills target and adds .exe on Windows for native entries', () => {
    const win = serverManifest(byId('server.gopls'), pins, { target: 'windows-x86_64', packRev: 3 });
    expect(win.version).toBe('0.19.1-3');
    expect(win.target).toBe('windows-x86_64');
    expect(win.launch.entry).toBe('gopls.exe');
    const mac = serverManifest(byId('server.rust-analyzer'), pins, { target: 'macos-aarch64' });
    expect(mac.version).toBe('2026.09.07-1');
    expect(mac.launch.entry).toBe('rust-analyzer');
    expect(serverPackName(mac)).toBe('server.rust-analyzer-2026.09.07-1-macos-aarch64.zip');
    expect(parsePackName(serverPackName(win))).toEqual({ id: 'server.gopls', version: '0.19.1-3', target: 'windows-x86_64' });
  });

  it('keeps `any` packs target-independent whatever --target says', () => {
    const m = serverManifest(byId('server.typescript-language-server'), pins, { target: 'macos-aarch64' });
    expect(m.target).toBe('any');
    expect(m.launch.entry).toBe('server/node_modules/typescript-language-server/lib/cli.mjs');
    expect(serverPackName(m)).toBe('server.typescript-language-server-6.0.0-1-any.zip');
  });

  it('guards template/pin drift and unknown targets', () => {
    const drifted = { ...byId('server.pyright'), version: '1.1.400-1' };
    expect(() => serverManifest(drifted, pins, { target: 'windows-x86_64' })).toThrow(/disagrees with pinned/);
    expect(() => serverManifest(byId('server.gopls'), pins, { target: 'linux-x86_64' })).toThrow(/unknown target/);
    expect(() => serverManifest({ id: 'server.nope', version: '1.0.0-1', target: 'any' }, pins, { target: 'windows-x86_64' })).toThrow(/no pin/);
  });
});

describe('selectServers + hostTarget', () => {
  it('resolves ids or bare names, all by default', () => {
    expect(selectServers(templates, 'all')).toHaveLength(5);
    expect(selectServers(templates, 'jdtls,server.gopls').map((m: { id: string }) => m.id)).toEqual(['server.jdtls', 'server.gopls']);
    expect(() => selectServers(templates, 'clangd')).toThrow(/no server manifest for "clangd"/);
  });

  it('maps the build machine to a pack target', () => {
    expect(hostTarget('win32', 'x64')).toBe('windows-x86_64');
    expect(hostTarget('darwin', 'arm64')).toBe('macos-aarch64');
    expect(hostTarget('linux', 'x64')).toBeNull();
  });
});

describe('node pack trimming', () => {
  it('drops docs, maps, tests and .bin shims; keeps code, d.ts and licenses', () => {
    expect(keepNodeFile('typescript/lib/tsserver.js')).toBe(true);
    expect(keepNodeFile('typescript/lib/lib.es2020.d.ts')).toBe(true);
    expect(keepNodeFile('typescript/LICENSE.txt')).toBe(true);
    expect(keepNodeFile('typescript/README.md')).toBe(false);
    expect(keepNodeFile('pyright/dist/pyright.js.map')).toBe(false);
    expect(keepNodeFile('.bin/pyright')).toBe(false);
    expect(keepNodeFile('foo/test/x.js')).toBe(false);
    expect(keepNodeFile('foo/docs/x.html')).toBe(false);
    expect(keepNodeFile('.package-lock.json')).toBe(false);
  });

  it('npmInstallArgs pins exact versions, prod only, into the stage prefix', () => {
    expect(npmInstallArgs('/stage/server', { a: '1.0.0', b: '2.0.0' })).toEqual([
      'install', '--prefix', '/stage/server', '--no-audit', '--no-fund', '--omit=dev', '--no-package-lock', 'a@1.0.0', 'b@2.0.0',
    ]);
    expect(goInstallArgs({ module: 'golang.org/x/tools/gopls', version: 'v0.19.1' })).toEqual(['install', 'golang.org/x/tools/gopls@v0.19.1']);
  });

  it('walkFiles lists regular files with posix paths and skips symlinks', () => {
    const root = mkdtempSync(join(tmpdir(), 'orrery-walk-'));
    mkdirSync(join(root, 'a', 'b'), { recursive: true });
    writeFileSync(join(root, 'a', 'b', 'c.js'), 'x');
    writeFileSync(join(root, 'top.js'), 'y');
    let linked = true;
    try {
      symlinkSync(join(root, 'top.js'), join(root, 'link.js'));
    } catch {
      linked = false; // no symlink privilege on this Windows box — nothing to skip
    }
    const names = walkFiles(root).map((f: { name: string }) => f.name);
    expect(names).toEqual(['a/b/c.js', 'top.js']);
    if (linked) expect(names).not.toContain('link.js');
  });
});

describe('packEntries', () => {
  it('manifest first, files sorted, modes forwarded', () => {
    const e = packEntries({ id: 'server.gopls' }, [{ name: 'gopls', data: Buffer.from('x'), mode: 0o755 }]);
    expect(e.map((x: { name: string }) => x.name)).toEqual(['manifest.json', 'gopls']);
    expect(e[1].mode).toBe(0o755);
    expect(JSON.parse(e[0].data).id).toBe('server.gopls');
  });
});
