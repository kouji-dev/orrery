import { Bridge } from '../data-source/bridge';
import { EntityStore } from './entity-store';

export interface FacadeConfig {
  listCommand: string;
  events: { created: string; updated?: string; deleted: string };
}

export interface EntityFacade {
  load(): Promise<void>;
  listen(): Promise<() => void>;
}

export function bindFacade<T extends { id: string }>(
  store: EntityStore<T>,
  bridge: Bridge,
  cfg: FacadeConfig,
): EntityFacade {
  return {
    async load() {
      store.setLoading(true);
      try {
        store.setAll(await bridge.invoke<T[]>(cfg.listCommand));
      } finally {
        store.setLoading(false);
      }
    },
    async listen() {
      const unsubs: Array<() => void> = [
        await bridge.on<T>(cfg.events.created, e => store.upsert(e)),
        await bridge.on<{ id: string }>(cfg.events.deleted, e => store.remove(e.id)),
      ];
      if (cfg.events.updated) {
        unsubs.push(await bridge.on<T>(cfg.events.updated, e => store.upsert(e)));
      }
      return () => unsubs.forEach(u => u());
    },
  };
}
