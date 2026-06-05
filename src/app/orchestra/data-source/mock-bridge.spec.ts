import { describe, it, expect, vi } from 'vitest';
import { MockBridge } from './mock-bridge';
import { Commands, Events } from './bridge';

describe('MockBridge', () => {
  it('lists seeded projects', async () => {
    const b = new MockBridge();
    const list = await b.invoke<any[]>(Commands.ProjectList);
    expect(list.length).toBeGreaterThan(0);
    expect(list[0]).toHaveProperty('id');
  });

  it('create adds a project and emits created', async () => {
    const b = new MockBridge();
    const seen: any[] = [];
    await b.on(Events.ProjectCreated, p => seen.push(p));
    const created = await b.invoke<any>(Commands.ProjectCreate, {
      req: { name: 'new', path: '~/x', icon: 'box', color: '#fff', withGit: false },
    });
    expect(created.id).toBeTruthy();
    expect(seen).toHaveLength(1);
    const list = await b.invoke<any[]>(Commands.ProjectList);
    expect(list.find(p => p.id === created.id)).toBeTruthy();
  });

  it('remove deletes and emits deleted', async () => {
    const b = new MockBridge();
    const created = await b.invoke<any>(Commands.ProjectCreate, {
      req: { name: 'tmp', path: '~/y', icon: 'box', color: '#fff', withGit: false },
    });
    const seen: any[] = [];
    await b.on(Events.ProjectDeleted, p => seen.push(p));
    await b.invoke(Commands.ProjectRemove, { id: created.id });
    expect(seen[0].id).toBe(created.id);
  });
});
