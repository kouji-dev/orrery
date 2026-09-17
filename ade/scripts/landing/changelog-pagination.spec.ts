import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { JSDOM } from 'jsdom';

const HERE = dirname(fileURLToPath(import.meta.url));
const HTML = readFileSync(resolve(HERE, '../../landing/changelog.html'), 'utf8');

// The page fetches the single-source changelog.json (hosted in orrery-releases)
// at runtime. jsdom has no fetch, so stub it with an inline fixture (5 entries >
// the page's PAGE_SIZE of 4, to exercise load-more). Entries carry the same
// shape changelog-json.mjs emits: tag/channel/date/ref/summary/commits[hash].
const DATA = [
  { tag: 'v0.9.4', channel: 'beta', date: 'June 18, 2026', ref: 'aaaaaaa', summary: 's', commits: [{ type: 'feat', hash: 'aaaaaaa', scope: 'x', msg: 'a' }] },
  { tag: 'v0.9.3', channel: 'beta', date: 'June 3, 2026', ref: 'bbbbbbb', summary: 's', commits: [{ type: 'fix', hash: 'bbbbbbb', scope: '', msg: 'b' }] },
  { tag: 'v0.9.2', channel: 'beta', date: 'May 22, 2026', ref: 'ccccccc', summary: 's', commits: [{ type: 'perf', hash: 'ccccccc', scope: '', msg: 'c' }] },
  { tag: 'v0.9.1', channel: 'dev', date: 'May 4, 2026', ref: 'ddddddd', summary: 's', commits: [{ type: 'feat', hash: 'ddddddd', scope: '', msg: 'd' }] },
  { tag: 'v0.9.0', channel: 'dev', date: 'April 18, 2026', ref: 'eeeeeee', summary: 's', commits: [{ type: 'feat', hash: 'eeeeeee', scope: '', msg: 'e' }] },
];

async function mount() {
  const dom = new JSDOM(HTML, {
    runScripts: 'dangerously',
    beforeParse(window) {
      (window as unknown as { fetch: () => Promise<unknown> }).fetch = () =>
        Promise.resolve({ ok: true, json: () => Promise.resolve(DATA) });
    },
  });
  // let the fetch().then(render) microtask chain settle
  await new Promise((r) => dom.window.setTimeout(r, 0));
  return dom.window.document;
}

describe('changelog load-more', () => {
  it('shows at most one page initially and reveals the button when there is more', async () => {
    const doc = await mount();
    const total = Number(doc.getElementById('relCount')!.textContent);
    const initial = doc.querySelectorAll('#releases .rel').length;
    expect(total).toBe(DATA.length);
    expect(initial).toBeGreaterThan(0);
    expect(initial).toBeLessThanOrEqual(total);
    const more = doc.getElementById('moreWrap') as HTMLElement;
    if (total > initial) expect(more.style.display).not.toBe('none');
  });

  it('reveals every release after clicking load-more to exhaustion', async () => {
    const doc = await mount();
    const total = Number(doc.getElementById('relCount')!.textContent);
    const btn = doc.getElementById('loadMore') as HTMLButtonElement;
    const wrap = () => doc.getElementById('moreWrap') as HTMLElement;
    let guard = 0;
    while (wrap().style.display !== 'none' && guard++ < 50) btn.click();
    expect(doc.querySelectorAll('#releases .rel').length).toBe(total);
    expect(wrap().style.display).toBe('none');
  });
});
