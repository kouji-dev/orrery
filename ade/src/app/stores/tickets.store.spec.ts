import { describe, it, expect, vi } from "vitest";
import { Injector, runInInjectionContext } from "@angular/core";
import { TicketsStore } from "./tickets.store";
import { BRIDGE, Bridge, Commands, Events } from "../data-source/bridge";
import { Ticket } from "../models";

function fakeBridge() {
  const invokes: { command: string; payload?: Record<string, unknown> }[] = [];
  const handlers: Record<string, (p: unknown) => void> = {};
  const bridge: Bridge = {
    invoke: vi.fn(<R>(command: string, payload?: Record<string, unknown>) => {
      invokes.push({ command, payload });
      return Promise.resolve(undefined as R);
    }),
    on: vi.fn((event: string, handler: (p: unknown) => void) => {
      handlers[event] = handler;
      return Promise.resolve(() => {});
    }),
    pickDirectory: vi.fn(() => Promise.resolve(null)),
  };
  return { bridge, invokes, handlers };
}

/** Flush enough microtask ticks for init() to complete: listen() (3 awaits) + load() (2 awaits). */
async function flushMicrotasks(ticks = 10) {
  for (let i = 0; i < ticks; i++) await Promise.resolve();
}

async function makeStore(bridge: Bridge): Promise<TicketsStore> {
  const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }] });
  const store = runInInjectionContext(injector, () => new TicketsStore());
  await flushMicrotasks();
  return store;
}

const TICKET: Ticket = {
  id: "t1",
  title: "Fix bug",
  notes: "",
  status: "todo",
  projectId: null,
  agentId: null,
  tags: [],
  createdAt: 1000,
  updatedAt: 1000,
};

describe("TicketsStore — init", () => {
  it("registers listeners for ticket:// events on construction", async () => {
    const { bridge } = fakeBridge();
    await makeStore(bridge);
    expect(bridge.on).toHaveBeenCalledWith(Events.TicketCreated, expect.any(Function));
    expect(bridge.on).toHaveBeenCalledWith(Events.TicketUpdated, expect.any(Function));
    expect(bridge.on).toHaveBeenCalledWith(Events.TicketDeleted, expect.any(Function));
  });

  it("invokes ticket_list on construction", async () => {
    const { bridge, invokes } = fakeBridge();
    await makeStore(bridge);
    const call = invokes.find((c) => c.command === Commands.TicketList);
    expect(call).toBeDefined();
  });
});

describe("TicketsStore — event-driven updates", () => {
  it("upserts a ticket when ticket://created fires", async () => {
    const { bridge, handlers } = fakeBridge();
    const store = await makeStore(bridge);

    handlers[Events.TicketCreated](TICKET);
    expect(store.all()).toContainEqual(TICKET);
  });

  it("upserts a ticket when ticket://updated fires", async () => {
    const { bridge, handlers } = fakeBridge();
    const store = await makeStore(bridge);

    handlers[Events.TicketCreated](TICKET);
    const updated = { ...TICKET, title: "Updated title" };
    handlers[Events.TicketUpdated](updated);
    expect(store.byId("t1")?.title).toBe("Updated title");
  });

  it("removes a ticket when ticket://deleted fires", async () => {
    const { bridge, handlers } = fakeBridge();
    const store = await makeStore(bridge);

    handlers[Events.TicketCreated](TICKET);
    handlers[Events.TicketDeleted]({ id: "t1" });
    expect(store.all()).not.toContainEqual(expect.objectContaining({ id: "t1" }));
  });
});

describe("TicketsStore — mutations", () => {
  it("create invokes ticket_create with req payload", async () => {
    const { bridge, invokes } = fakeBridge();
    const store = await makeStore(bridge);

    store.create({ title: "New ticket", notes: "some notes", projectId: null });
    const call = invokes.find((c) => c.command === Commands.TicketCreate);
    expect(call?.payload).toEqual({
      req: { title: "New ticket", notes: "some notes", projectId: null },
    });
  });

  it("update invokes ticket_update with id and req", async () => {
    const { bridge, invokes } = fakeBridge();
    const store = await makeStore(bridge);

    store.update("t1", { title: "Renamed" });
    const call = invokes.find((c) => c.command === Commands.TicketUpdate);
    expect(call?.payload).toEqual({ id: "t1", req: { title: "Renamed" } });
  });

  it("remove invokes ticket_remove with id", async () => {
    const { bridge, invokes } = fakeBridge();
    const store = await makeStore(bridge);

    store.remove("t1");
    const call = invokes.find((c) => c.command === Commands.TicketRemove);
    expect(call?.payload).toEqual({ id: "t1" });
  });

  it("setStatus invokes ticket_set_status with id and status", async () => {
    const { bridge, invokes } = fakeBridge();
    const store = await makeStore(bridge);

    store.setStatus("t1", "inprogress");
    const call = invokes.find((c) => c.command === Commands.TicketSetStatus);
    expect(call?.payload).toEqual({ id: "t1", status: "inprogress" });
  });

  it("addComment invokes comment_add with ticketId and req", async () => {
    const { bridge, invokes } = fakeBridge();
    const store = await makeStore(bridge);

    store.addComment("t1", { body: "Great work!", author: "user", role: "user" });
    const call = invokes.find((c) => c.command === Commands.CommentAdd);
    expect(call?.payload).toEqual({
      ticketId: "t1",
      req: { body: "Great work!", author: "user", role: "user" },
    });
  });

  it("comments invokes comment_list with ticketId", async () => {
    const { bridge, invokes } = fakeBridge();
    const store = await makeStore(bridge);

    store.comments("t1");
    const call = invokes.find((c) => c.command === Commands.CommentList);
    expect(call?.payload).toEqual({ ticketId: "t1" });
  });
});
