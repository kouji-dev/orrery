import { inject, Injectable } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { Comment, Ticket, TicketStatus } from "../models";
import { bindFacade } from "../state/entity-facade";
import { createEntityStore } from "../state/entity-store";

@Injectable({ providedIn: "root" })
export class TicketsStore {
  private bridge = inject(BRIDGE);
  private store = createEntityStore<Ticket>((t) => t.id);

  readonly all = this.store.all;
  readonly loading = this.store.loading;

  private facade = bindFacade(this.store, this.bridge, {
    listCommand: Commands.TicketList,
    events: {
      created: Events.TicketCreated,
      updated: Events.TicketUpdated,
      deleted: Events.TicketDeleted,
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

  byId(id: string): Ticket | undefined {
    return this.all().find((t) => t.id === id);
  }

  // ---- mutations: invoke only; the store updates from ticket:// events (single source of truth) ----

  async create(req: {
    title: string;
    notes?: string;
    projectId?: string | null;
    tags?: string[];
  }): Promise<Ticket> {
    return this.bridge.invoke<Ticket>(Commands.TicketCreate, { req });
  }

  async update(
    id: string,
    patch: { title?: string; notes?: string; projectId?: string | null; tags?: string[] },
  ): Promise<Ticket> {
    return this.bridge.invoke<Ticket>(Commands.TicketUpdate, { id, req: patch });
  }

  async remove(id: string): Promise<void> {
    await this.bridge.invoke(Commands.TicketRemove, { id });
  }

  async setStatus(id: string, status: TicketStatus): Promise<Ticket> {
    return this.bridge.invoke<Ticket>(Commands.TicketSetStatus, { id, status });
  }

  comments(ticketId: string): Promise<Comment[]> {
    return this.bridge.invoke<Comment[]>(Commands.CommentList, { ticketId });
  }

  async addComment(
    ticketId: string,
    req: { body: string; author?: string; role?: string; tool?: string | null },
  ): Promise<Comment> {
    return this.bridge.invoke<Comment>(Commands.CommentAdd, { ticketId, req });
  }
}
