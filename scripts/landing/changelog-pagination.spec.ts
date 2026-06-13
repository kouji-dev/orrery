import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { JSDOM } from 'jsdom';

const HTML = readFileSync(
  resolve(dirname(fileURLToPath(import.meta.url)), '../../landing/changelog.html'),
  'utf8',
);

function mount() {
  const dom = new JSDOM(HTML, { runScripts: 'dangerously' });
  return dom.window.document;
}

describe('changelog load-more', () => {
  it('shows at most one page initially and reveals the button when there is more', () => {
    const doc = mount();
    const total = Number(doc.getElementById('relCount').textContent);
    const initial = doc.querySelectorAll('#releases .rel').length;
    expect(initial).toBeGreaterThan(0);
    expect(initial).toBeLessThanOrEqual(total);
    const more = doc.getElementById('moreWrap') as HTMLElement;
    if (total > initial) expect(more.style.display).not.toBe('none');
  });

  it('reveals every release after clicking load-more to exhaustion', () => {
    const doc = mount();
    const total = Number(doc.getElementById('relCount').textContent);
    const btn = doc.getElementById('loadMore') as HTMLButtonElement;
    const wrap = () => doc.getElementById('moreWrap') as HTMLElement;
    let guard = 0;
    while (wrap().style.display !== 'none' && guard++ < 50) btn.click();
    expect(doc.querySelectorAll('#releases .rel').length).toBe(total);
    expect(wrap().style.display).toBe('none');
  });
});
