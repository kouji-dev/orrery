import { inject, Injectable } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { Project } from "../models";
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
    events: { created: Events.ProjectCreated, deleted: Events.ProjectDeleted },
  });

  constructor() {
    void this.init();
  }
  private async init() {
    await this.facade.listen();
    await this.facade.load();
  }

  byId(id: string): Project | undefined {
    return this.all().find((p) => p.id === id);
  }
  async create(req: {
    name: string;
    path: string;
    icon: string;
    color: string;
    withGit: boolean;
  }): Promise<Project> {
    const p = await this.bridge.invoke<Project>(Commands.ProjectCreate, { req });
    this.store.upsert(p);
    return p;
  }
  async remove(id: string): Promise<void> {
    await this.bridge.invoke(Commands.ProjectRemove, { id });
    this.store.remove(id);
  }
  detectGit(path: string): Promise<boolean> {
    return this.bridge.invoke<boolean>(Commands.ProjectDetectGit, { path });
  }
}
