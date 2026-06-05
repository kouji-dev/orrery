import { describe, it, expect } from 'vitest';
import { createEntityStore } from './entity-store';

interface P { id: string; name: string; }

describe('createEntityStore', () => {
  it('setAll establishes entities + order', () => {
    const s = createEntityStore<P>(p => p.id);
    s.setAll([{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    expect(s.ids()).toEqual(['a', 'b']);
    expect(s.all().map(p => p.name)).toEqual(['A', 'B']);
  });

  it('upsert adds new and replaces existing without duplicating ids', () => {
    const s = createEntityStore<P>(p => p.id);
    s.upsert({ id: 'a', name: 'A' });
    s.upsert({ id: 'a', name: 'A2' });
    expect(s.ids()).toEqual(['a']);
    expect(s.all()[0].name).toBe('A2');
  });

  it('update patches a field', () => {
    const s = createEntityStore<P>(p => p.id);
    s.upsert({ id: 'a', name: 'A' });
    s.update('a', { name: 'Z' });
    expect(s.all()[0].name).toBe('Z');
  });

  it('remove drops entity and id', () => {
    const s = createEntityStore<P>(p => p.id);
    s.setAll([{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    s.remove('a');
    expect(s.ids()).toEqual(['b']);
  });

  it('active reflects setActive', () => {
    const s = createEntityStore<P>(p => p.id);
    s.setAll([{ id: 'a', name: 'A' }]);
    expect(s.active()).toBeNull();
    s.setActive('a');
    expect(s.active()?.name).toBe('A');
  });
});
