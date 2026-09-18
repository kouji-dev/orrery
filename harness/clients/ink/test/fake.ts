/**
 * A fixture-backed fake transport.
 *
 * Nothing here talks to a network or to a model: every frame comes from
 * `harness/clients/conformance`, the same files the Rust SDK replays.
 */

import { join } from "node:path";

import { conformance, type Frame } from "@orrery/client";

import type { Connection } from "../src/app.js";

/** One scenario by file name, without the extension. */
export function scenario(name: string): conformance.Scenario {
  return conformance.load(join(conformance.fixturesDir(), `${name}.jsonl`));
}

/** Every frame of a scenario, checkpoints dropped. */
export function frames(name: string): Frame[] {
  return scenario(name)
    .steps.filter((s): s is Extract<conformance.Step, { kind: "event" }> => s.kind === "event")
    .map((s) => s.frame);
}

/** The same frames, renumbered so two scenarios can be played back to back. */
export function offset(list: Frame[], by: number): Frame[] {
  return list.map((f) => ({ ...f, seq: f.seq + by }));
}

/** What a component asked the session to do. */
export interface Call {
  what: "attach" | "submit" | "cancel" | "answer" | "intent";
  args: unknown[];
}

/** A `Connection` over a canned list of frames. */
export class FakeConnection implements Connection {
  readonly calls: Call[] = [];
  /** Resolves once the app has drained the frame list. */
  private release: (() => void) | null = null;

  constructor(private readonly list: Frame[]) {}

  /** Yield one frame per macrotask, so Ink renders in between. */
  async *frames(): AsyncIterable<Frame> {
    for (const frame of this.list) {
      await new Promise((r) => setTimeout(r, 0));
      yield frame;
    }
    // Stay open the way a live SSE stream does: the app must not depend on the
    // stream ending to be usable.
    await new Promise<void>((resolve) => {
      this.release = resolve;
    });
  }

  /** Close the stream, as a server hang-up would. */
  close(): void {
    this.release?.();
    this.release = null;
  }

  async attach(session: string, since?: number): Promise<void> {
    this.calls.push({ what: "attach", args: [session, since] });
  }

  async submit(text: string): Promise<string> {
    this.calls.push({ what: "submit", args: [text] });
    return "turn-fake";
  }

  async cancel(turn: string): Promise<void> {
    this.calls.push({ what: "cancel", args: [turn] });
  }

  async answer(prompt: string, answer: string): Promise<void> {
    this.calls.push({ what: "answer", args: [prompt, answer] });
  }

  async intent(surface: string, value: unknown): Promise<void> {
    this.calls.push({ what: "intent", args: [surface, value] });
  }

  /** Every call of one kind, oldest first. */
  of(what: Call["what"]): Call[] {
    return this.calls.filter((c) => c.what === what);
  }
}

/** Let Ink's render loop and the frame pump run. */
export async function settle(ticks = 8): Promise<void> {
  for (let i = 0; i < ticks; i += 1) {
    await new Promise((r) => setTimeout(r, 4));
  }
}
