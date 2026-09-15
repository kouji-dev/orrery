import { ApplicationRef, provideZonelessChangeDetection, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { LspServer, LspStatus } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { leaf, PaneNode, group } from "../workspace/pane-model";
import { DOC_SYNC_DEBOUNCE_MS, LspDocSyncService, openFileKeys } from "./lsp-doc-sync.service";
import { LspStatusStore } from "./lsp-status.store";

const A = "agent-1";
const JAVA = "src/Main.java";
const RS = "src/lib.rs";

function server(over: Partial<LspServer> & { id: string }): LspServer {
  const [extId, projectId] = over.id.split(":");
  return {
    extId,
    label: extId.replace(/^server\./, ""),
    language: "java",
    root: "C:/p",
    projectId,
    projectName: projectId,
    pid: 100,
    state: "ready",
    memBytes: 0,
    cpu: 0,
    restarts: 0,
    startedAt: 1,
    lastError: null,
    ...over,
  };
}

describe("openFileKeys", () => {
  it("walks every leaf of every root, dedupes, skips unassigned leaves and virtual uris", () => {
    const a: PaneNode = { ...leaf(A, "file"), files: [JAVA, "jdt://contents/x.class", RS] };
    const b: PaneNode = { ...leaf(A, "file"), files: [JAVA] };
    const c: PaneNode = { ...leaf(null, "file"), files: ["ignored.ts"] };
    const d: PaneNode = { ...leaf("agent-2", "terminal"), files: ["other.go"] };
    const roots = { t1: group(a, b), t2: group(c, d) };
    expect(openFileKeys(roots)).toEqual([
      { id: A, path: JAVA },
      { id: A, path: RS },
      { id: "agent-2", path: "other.go" },
    ]);
  });
});

describe("LspDocSyncService", () => {
  let invoke: ReturnType<typeof vi.fn>;
  let handlers: Array<(p: unknown) => void>;
  let edits: EditsStore;
  let paneRoots: ReturnType<typeof signal<Record<string, PaneNode>>>;
  let agents: ReturnType<typeof signal<{ id: string; projectId: string }[]>>;
  const tick = () => TestBed.inject(ApplicationRef).tick();
  const docCalls = () => invoke.mock.calls.filter((c) => String(c[0]).startsWith("lsp_doc_")).map((c) => [c[0], c[1]]);
  const push = (status: LspStatus) => {
    for (const h of handlers) h(status);
    tick();
  };

  beforeEach(async () => {
    vi.useFakeTimers();
    invoke = vi.fn(async (cmd: string) => (cmd === "lsp_status" ? { servers: [] } : null));
    handlers = [];
    paneRoots = signal<Record<string, PaneNode>>({});
    agents = signal<{ id: string; projectId: string }[]>([{ id: A, projectId: "p1" }]);
    const bridge: Bridge = {
      invoke: invoke as unknown as Bridge["invoke"],
      on: async (event, handler) => {
        if (event === "lsp://status") handlers.push(handler as (p: unknown) => void);
        return () => {};
      },
      pickDirectory: async () => null,
      pickFile: async () => null,
    };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: BRIDGE, useValue: bridge },
        { provide: UiStore, useValue: { flash: vi.fn(), paneRoots } },
        { provide: AgentsStore, useValue: { all: agents } },
        { provide: ExtensionsStore, useValue: { languagesOfServer: (id: string) => (id === "server.tsls" ? ["javascript", "typescript"] : []), servers: () => [] } },
      ],
    });
    TestBed.inject(LspStatusStore);
    await vi.advanceTimersByTimeAsync(0); // the seed
    TestBed.inject(LspDocSyncService).start();
    edits = TestBed.inject(EditsStore);
    tick();
  });
  afterEach(() => vi.useRealTimers());

  const openTab = (path: string) => {
    paneRoots.set({ t1: { ...leaf(A, "file"), files: [path] } });
    tick();
  };

  it("sends nothing while no server answers for the file's language in that project", () => {
    edits.open(A, JAVA, "class Main {}");
    openTab(JAVA);
    expect(docCalls()).toEqual([]);
    // a server for ANOTHER project is not it either
    push({ servers: [server({ id: "server.jdtls:p2" })] });
    expect(docCalls()).toEqual([]);
    // nor a crashed one for this project
    push({ servers: [server({ id: "server.jdtls:p1", state: "crashed" })] });
    expect(docCalls()).toEqual([]);
  });

  it("opens once the server is live, with the buffer text and the language id", () => {
    edits.open(A, JAVA, "class Main {}");
    openTab(JAVA);
    push({ servers: [server({ id: "server.jdtls:p1", state: "starting" })] });
    expect(docCalls()).toEqual([["lsp_doc_open", { id: A, path: JAVA, text: "class Main {}", languageId: "java" }]]);
    // a fresh push with the same list does not re-open
    push({ servers: [server({ id: "server.jdtls:p1", state: "ready" })] });
    expect(docCalls().length).toBe(1);
  });

  it("waits for the buffer: a tab whose editor has not adopted the text yet opens later", () => {
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    openTab(JAVA);
    expect(docCalls()).toEqual([]);
    edits.open(A, JAVA, "x");
    edits.update(A, JAVA, "xy"); // any tick re-runs the effect
    tick();
    expect(docCalls()).toEqual([["lsp_doc_open", { id: A, path: JAVA, text: "xy", languageId: "java" }]]);
  });

  it("debounces keystrokes into one didChange per 150 ms with a monotonic version", async () => {
    edits.open(A, JAVA, "a");
    openTab(JAVA);
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    edits.update(A, JAVA, "ab");
    tick();
    edits.update(A, JAVA, "abc");
    tick();
    await vi.advanceTimersByTimeAsync(DOC_SYNC_DEBOUNCE_MS - 20);
    expect(docCalls().length).toBe(1); // still only the open
    await vi.advanceTimersByTimeAsync(30);
    expect(docCalls()[1]).toEqual(["lsp_doc_change", { id: A, path: JAVA, text: "abc", version: 2 }]);
    edits.update(A, JAVA, "abcd");
    tick();
    await vi.advanceTimersByTimeAsync(DOC_SYNC_DEBOUNCE_MS + 10);
    expect(docCalls()[2]).toEqual(["lsp_doc_change", { id: A, path: JAVA, text: "abcd", version: 3 }]);
    expect(docCalls().length).toBe(3);
  });

  it("closes when the tab disappears, and a pending change is dropped with it", async () => {
    edits.open(A, JAVA, "a");
    openTab(JAVA);
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    edits.update(A, JAVA, "ab");
    tick();
    paneRoots.set({});
    tick();
    await vi.advanceTimersByTimeAsync(DOC_SYNC_DEBOUNCE_MS + 10);
    expect(docCalls().slice(1)).toEqual([["lsp_doc_close", { id: A, path: JAVA }]]);
  });

  it("a server going away forgets the file silently; its return re-opens it", () => {
    edits.open(A, JAVA, "a");
    openTab(JAVA);
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    push({ servers: [] });
    expect(docCalls().length).toBe(1);
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    expect(docCalls().length).toBe(2);
    expect(docCalls()[1][0]).toBe("lsp_doc_open");
  });

  it("matches the pack's language list, not only the instance's language", () => {
    edits.open(A, "src/a.ts", "let a = 1;");
    openTab("src/a.ts");
    push({ servers: [server({ id: "server.tsls:p1", language: "typescript" })] });
    expect(docCalls()).toEqual([["lsp_doc_open", { id: A, path: "src/a.ts", text: "let a = 1;", languageId: "javascript" }]]);
  });

  it("a project tab (pseudo-agent id = project id) resolves to itself", () => {
    agents.set([]);
    paneRoots.set({ t1: { ...leaf("p1", "file"), files: [JAVA] } });
    edits.open("p1", JAVA, "x");
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    expect(docCalls()).toEqual([["lsp_doc_open", { id: "p1", path: JAVA, text: "x", languageId: "java" }]]);
  });

  it("virtual uris are never synced", () => {
    edits.open(A, "jdt://contents/List.class", "class List {}");
    openTab("jdt://contents/List.class");
    push({ servers: [server({ id: "server.jdtls:p1" })] });
    expect(docCalls()).toEqual([]);
  });
});
