import { inject, Injectable } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { Commit, Project } from "../models";
import { bindFacade } from "../state/entity-facade";
import { createEntityStore } from "../state/entity-store";

@Injectable({ providedIn: "root" })
export class ProjectsStore {
  private bridge = inject(BRIDGE);
  private store = createEntityStore<Project>((p) => p.id);

  readonly all = this.store.all;
  readonly loading = this.store.loading;

  private facade = bindFacade(this.store, this.bridge, {
    listCommand: Commands.ProjectList,
    events: {
      created: Events.ProjectCreated,
      updated: Events.ProjectUpdated,
      deleted: Events.ProjectDeleted,
    },
  });

  constructor() {
    void this.init();
  }
  private async init() {
    try {
      await this.facade.listen();
      await this.facade.load();
    } catch {
      // backend unavailable (e.g. opened outside the Tauri shell) — start empty
    }
  }

  byId(id: string): Project | undefined {
    return this.all().find((p) => p.id === id);
  }
  // ---- mutations: invoke only; the store updates from project:// events (single source of truth) ----
  async create(req: {
    name: string;
    path: string;
    icon: string;
    color: string;
    withGit: boolean;
  }): Promise<Project> {
    // backend persists + emits project://created → facade upserts. Returned for the caller's toast.
    return this.bridge.invoke<Project>(Commands.ProjectCreate, { req });
  }
  async update(
    id: string,
    patch: { name?: string; path?: string; icon?: string; color?: string },
  ): Promise<Project> {
    // backend emits project://updated → facade upserts the enriched project.
    return this.bridge.invoke<Project>(Commands.ProjectUpdate, { id, req: patch });
  }
  initGit(id: string): Promise<Project> {
    // backend emits project://updated → facade upserts (hasGit flips true).
    return this.bridge.invoke<Project>(Commands.ProjectInitGit, { id });
  }
  async remove(id: string): Promise<void> {
    // backend emits project://deleted → facade drops it.
    await this.bridge.invoke(Commands.ProjectRemove, { id });
  }
  detectGit(path: string): Promise<boolean> {
    return this.bridge.invoke<boolean>(Commands.ProjectDetectGit, { path });
  }
  commits(id: string, limit?: number): Promise<Commit[]> {
    return this.bridge.invoke<Commit[]>(Commands.ProjectCommits, { id, limit });
  }
  pickDirectory(defaultPath?: string): Promise<string | null> {
    return this.bridge.pickDirectory(defaultPath);
  }
}
