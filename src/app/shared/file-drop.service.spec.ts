import { Injector, runInInjectionContext } from "@angular/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TerminalService } from "../terminal.service";
import { FileDropService } from "./file-drop.service";

interface TermStub {
  focusedAgentId: () => string | null;
  pasteToAgent: ReturnType<typeof vi.fn>;
}

function make(term: Partial<TermStub> = {}): { svc: FileDropService; term: TermStub } {
  const termStub: TermStub = {
    focusedAgentId: () => null,
    pasteToAgent: vi.fn(() => true),
    ...term,
  };
  const injector = Injector.create({
    providers: [{ provide: TerminalService, useValue: termStub }],
  });
  const svc = runInInjectionContext(injector, () => new FileDropService());
  return { svc, term: termStub };
}

/** jsdom in this project doesn't implement elementFromPoint — install a mock
 *  returning `el`, and hand back the spy so tests can assert on calls. */
function stubPoint(el: Element | null): ReturnType<typeof vi.fn> {
  const fn = vi.fn().mockReturnValue(el);
  (document as unknown as { elementFromPoint: unknown }).elementFromPoint = fn;
  return fn;
}

afterEach(() => {
  delete (document as unknown as { elementFromPoint?: unknown }).elementFromPoint;
  document.body.replaceChildren();
});

describe("FileDropService.route", () => {
  it("inserts the path into a textarea at the caret and fires input", () => {
    const ta = document.createElement("textarea");
    ta.value = "before after";
    document.body.appendChild(ta);
    ta.setSelectionRange(7, 7); // caret between "before " and "after"
    stubPoint(ta);
    const onInput = vi.fn();
    ta.addEventListener("input", onInput);

    const { svc, term } = make();
    svc.route(["/home/u/x.ts"], 10, 10);

    expect(ta.value).toBe("before /home/u/x.ts after");
    expect(onInput).toHaveBeenCalledOnce();
    expect(term.pasteToAgent).not.toHaveBeenCalled();
  });

  it("appends at end when the field has no active selection range", () => {
    const ta = document.createElement("textarea");
    ta.value = "hi ";
    document.body.appendChild(ta);
    stubPoint(ta);

    const { svc } = make();
    svc.route(["/a.ts"], 0, 0);
    expect(ta.value).toBe("hi /a.ts ");
  });

  it("pastes into the agent terminal under the drop point (data-agent-id)", () => {
    const host = document.createElement("div");
    host.setAttribute("data-agent-id", "agent-7");
    const inner = document.createElement("canvas"); // xterm renders into a canvas
    host.appendChild(inner);
    document.body.appendChild(host);
    stubPoint(inner);

    const { svc, term } = make({ focusedAgentId: () => "other" });
    svc.route(["/proj/main.rs"], 5, 5);

    expect(term.pasteToAgent).toHaveBeenCalledWith("agent-7", "/proj/main.rs ");
  });

  it("falls back to the focused terminal when the drop is over neither", () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    stubPoint(div);

    const { svc, term } = make({ focusedAgentId: () => "focused-1" });
    svc.route(["/x"], 0, 0);
    expect(term.pasteToAgent).toHaveBeenCalledWith("focused-1", "/x ");
  });

  it("does nothing when nothing is focused and the drop misses every surface", () => {
    stubPoint(null);
    const { svc, term } = make({ focusedAgentId: () => null });
    svc.route(["/x"], 0, 0);
    expect(term.pasteToAgent).not.toHaveBeenCalled();
  });

  it("joins multiple paths with quoting + a trailing space", () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    stubPoint(div);

    const { svc, term } = make({ focusedAgentId: () => "a" });
    svc.route(["/nospace.ts", "/with space/f.ts"], 0, 0);
    expect(term.pasteToAgent).toHaveBeenCalledWith("a", '/nospace.ts "/with space/f.ts" ');
  });

  it("ignores an empty path list without touching the DOM", () => {
    const spy = stubPoint(null);
    const { svc, term } = make();
    svc.route([], 0, 0);
    expect(spy).not.toHaveBeenCalled();
    expect(term.pasteToAgent).not.toHaveBeenCalled();
  });
});
