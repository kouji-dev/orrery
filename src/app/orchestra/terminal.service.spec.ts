import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { AgentsStore } from "./stores/agents.store";
import { TerminalService } from "./terminal.service";

// TerminalService only calls AgentsStore from xterm event handlers (input/resize),
// none of which fire during a write/tail test — a bare stub is enough. We build it
// via a plain Injector + runInInjectionContext so no TestBed/zone bootstrap is
// needed (this repo's vitest setup has no Angular test environment).
const stubStore = {
  input: () => Promise.resolve(),
  resize: () => Promise.resolve(),
} as unknown as AgentsStore;

function makeService(): TerminalService {
  const injector = Injector.create({
    providers: [{ provide: AgentsStore, useValue: stubStore }],
  });
  return runInInjectionContext(injector, () => new TerminalService());
}

// `write`'s callback fires once a chunk is fully parsed; a trailing empty write
// flushes the queue so the buffer is current before we read tail().
function flush(svc: TerminalService, id: string): Promise<void> {
  return new Promise((r) => {
    // @ts-expect-error reach the underlying term to chain a flush callback
    svc["handle"](id).term.write("", () => r());
  });
}

describe("TerminalService.tail", () => {
  it("returns [] when no terminal exists yet", () => {
    expect(makeService().tail("nope", 3)).toEqual([]);
  });

  it("reads the final rendered text, not stale frames", async () => {
    const svc = makeService();
    // a \r overwrite (progress bar) then a clear-line redraw of the same row
    svc.write("a", "10%\r50%\r100% done\r\n");
    svc.write("a", "downloading\x1b[2K\rdownloaded\r\n");
    await flush(svc, "a");
    // tail sees only the final state of each row — no "10%"/"50%"/"downloading"
    expect(svc.tail("a", 3)).toEqual(["100% done", "downloaded"]);
  });

  it("takes the last n non-empty rows", async () => {
    const svc = makeService();
    svc.write("b", "one\r\ntwo\r\n\r\nthree\r\nfour\r\n");
    await flush(svc, "b");
    expect(svc.tail("b", 2)).toEqual(["three", "four"]);
  });
});
