import { Bridge, Commands, Events } from './bridge';

// Seed mirrors the frontend Project shape (extra fields like branches/files
// are kept here only for the browser/mock so the rest of the UI still works).
const SEED = [
  { id: 'p_pay', name: 'payments-service', path: '~/code/northwind/payments-service',
    icon: 'box', color: '#a855f7', hasGit: true, branch: 'main', head: 'a3f91c2',
    branches: ['main', 'develop', 'release/2.4', 'hotfix/refund-rounding'], files: [] },
  { id: 'p_web', name: 'web-dashboard', path: '~/code/northwind/web-dashboard',
    icon: 'globe', color: '#22d3ee', hasGit: true, branch: 'main', head: '7d10b4e',
    branches: ['main', 'develop', 'feat/new-settings'], files: [] },
  { id: 'p_infra', name: 'infra-terraform', path: '~/code/northwind/infra-terraform',
    icon: 'server', color: '#34e0a1', hasGit: true, branch: 'main', head: 'f02ce91',
    branches: ['main', 'staging'], files: [] },
];

export class MockBridge implements Bridge {
  private projects: any[] = SEED.map(p => ({ ...p }));
  private listeners = new Map<string, Set<(p: any) => void>>();

  async invoke<R>(command: string, payload: any = {}): Promise<R> {
    switch (command) {
      case Commands.ProjectList:
        return this.projects.slice() as R;
      case Commands.ProjectDetectGit:
        return (/(\/code\/|github|\.git)/i.test(payload.path) && payload.path.length > 4) as unknown as R;
      case Commands.ProjectCreate: {
        const r = payload.req;
        const detected = /(\/code\/|github|\.git)/i.test(r.path);
        const hasGit = r.withGit || detected;
        const p = {
          id: crypto.randomUUID(),
          name: r.name, path: r.path, icon: r.icon, color: r.color,
          hasGit, branch: hasGit ? 'main' : null, head: hasGit ? '0000000' : null,
          branches: ['main'], files: [],
        };
        this.projects.push(p);
        this.emit(Events.ProjectCreated, p);
        return p as R;
      }
      case Commands.ProjectRemove: {
        this.projects = this.projects.filter(p => p.id !== payload.id);
        this.emit(Events.ProjectDeleted, { id: payload.id });
        return undefined as R;
      }
      default:
        throw new Error(`MockBridge: unhandled command ${command}`);
    }
  }

  async on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    const set = this.listeners.get(event) ?? new Set();
    set.add(handler as (p: any) => void);
    this.listeners.set(event, set);
    return () => set.delete(handler as (p: any) => void);
  }

  private emit(event: string, payload: any) {
    this.listeners.get(event)?.forEach(h => h(payload));
  }
}
