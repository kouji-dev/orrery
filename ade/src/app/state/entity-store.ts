import { computed, signal, Signal } from '@angular/core';

export interface EntityStore<T> {
  entities: Signal<Record<string, T>>;
  ids: Signal<string[]>;
  all: Signal<T[]>;
  loading: Signal<boolean>;
  active: Signal<T | null>;
  byId(id: string): Signal<T | undefined>;
  setAll(list: T[]): void;
  upsert(e: T): void;
  upsertMany(list: T[]): void;
  update(id: string, patch: Partial<T>): void;
  remove(id: string): void;
  setActive(id: string | null): void;
  setLoading(v: boolean): void;
  reset(): void;
}

export function createEntityStore<T>(idOf: (e: T) => string): EntityStore<T> {
  const entities = signal<Record<string, T>>({});
  const ids = signal<string[]>([]);
  const loading = signal(false);
  const activeId = signal<string | null>(null);

  const all = computed(() => ids().map(id => entities()[id]));
  const active = computed(() => {
    const id = activeId();
    return id ? entities()[id] ?? null : null;
  });

  return {
    entities, ids, all, loading, active,
    byId: (id: string) => computed(() => entities()[id]),
    setAll(list) {
      const map: Record<string, T> = {};
      const order: string[] = [];
      for (const e of list) { const id = idOf(e); map[id] = e; order.push(id); }
      entities.set(map); ids.set(order);
    },
    upsert(e) {
      const id = idOf(e);
      entities.update(m => ({ ...m, [id]: e }));
      ids.update(arr => (arr.includes(id) ? arr : [...arr, id]));
    },
    upsertMany(list) { for (const e of list) this.upsert(e); },
    update(id, patch) {
      entities.update(m => (m[id] ? { ...m, [id]: { ...m[id], ...patch } } : m));
    },
    remove(id) {
      entities.update(m => { const { [id]: _drop, ...rest } = m; return rest; });
      ids.update(arr => arr.filter(x => x !== id));
    },
    setActive(id) { activeId.set(id); },
    setLoading(v) { loading.set(v); },
    reset() { entities.set({}); ids.set([]); activeId.set(null); loading.set(false); },
  };
}
