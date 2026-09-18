#!/usr/bin/env node
/**
 * The entry point: `orrery --ui ink` spawns this, and a third party runs the
 * same file by hand against `orrery serve`.
 *
 * Everything it needs arrives in two ways and no others — `--endpoint` or
 * `ORRERY_ENDPOINT` — because that is the whole contract a third-party client
 * has to implement. See the README.
 */

import { pathToFileURL } from "node:url";

import { render } from "ink";

import { AguiSession, type Frame } from "@orrery/client";

import { App, type Connection } from "./app.js";

export { App, type AppProps, type Connection } from "./app.js";
export { registerRenderer, rendererFor, registered, type CustomProps, type CustomRenderer } from "./registry.js";
export { SurfaceNode, COMPONENTS, type Drawable, type SurfaceProps } from "./surfaces/index.js";

/** The lowest Node this client runs on. Ink 5 and vitest 4 both want it. */
export const NODE_FLOOR = "20.19.0";

/** Compare two dotted versions. */
function older(a: string, b: string): boolean {
  const left = a.split(".").map((n) => Number.parseInt(n, 10) || 0);
  const right = b.split(".").map((n) => Number.parseInt(n, 10) || 0);
  for (let i = 0; i < 3; i += 1) {
    if ((left[i] ?? 0) !== (right[i] ?? 0)) return (left[i] ?? 0) < (right[i] ?? 0);
  }
  return false;
}

/** A message a person can act on, rather than a stack trace. */
export class Misconfigured extends Error {}

/** Refuse to start on a Node too old for Ink, saying so plainly. */
export function checkNode(version: string = process.versions.node): void {
  if (older(version, NODE_FLOOR)) {
    throw new Misconfigured(
      `this client needs Node ${NODE_FLOOR} or newer (found ${version}).\n` +
        `Install a newer Node, or run \`orrery\` without \`--ui ink\` for the built-in TUI.`,
    );
  }
}

/** `--endpoint URL`, else `$ORRERY_ENDPOINT`, else a message that says what to do. */
export function resolveEndpoint(argv: string[], env: NodeJS.ProcessEnv): string {
  const at = argv.indexOf("--endpoint");
  const flag = at >= 0 ? argv[at + 1] : undefined;
  const inline = argv.find((a) => a.startsWith("--endpoint="))?.slice("--endpoint=".length);
  const endpoint = flag ?? inline ?? env["ORRERY_ENDPOINT"];
  if (!endpoint) {
    throw new Misconfigured(
      "no endpoint. Pass `--endpoint http://127.0.0.1:PORT` or set `ORRERY_ENDPOINT`.\n" +
        "`orrery serve` prints one; `orrery --ui ink` sets it for you.",
    );
  }
  return endpoint;
}

/** `--session ID`, else `$ORRERY_SESSION`, else the kernel's default session. */
export function resolveSession(argv: string[], env: NodeJS.ProcessEnv): string {
  const at = argv.indexOf("--session");
  return (at >= 0 ? argv[at + 1] : undefined) ?? env["ORRERY_SESSION"] ?? "default";
}

/** A `Connection` that also knows how to stop its own pump. */
export interface ProcessConnection extends Connection {
  /** Close the frame stream, so `<App>` can unmount. */
  close(): void;
}

/**
 * A `Connection` over an `AguiSession`.
 *
 * AG-UI's input path is a run invocation, so a turn's frames arrive as the
 * result of starting it: `POST /run`, parsed as SSE, by the SDK. The queue is
 * what turns those per-run generators into the single stream `<App>`
 * subscribes to once. Nothing here reaches past `@orrery/client`.
 */
export function sessionConnection(session: AguiSession): ProcessConnection {
  const waiting: Frame[] = [];
  let wake: (() => void) | null = null;
  let open = true;

  const push = (frame: Frame): void => {
    waiting.push(frame);
    const resume = wake;
    wake = null;
    resume?.();
  };

  return {
    async *frames(): AsyncIterable<Frame> {
      while (open) {
        while (waiting.length > 0) yield waiting.shift()!;
        if (!open) return;
        await new Promise<void>((resolve) => {
          wake = resolve;
        });
      }
    },
    close(): void {
      open = false;
      const resume = wake;
      wake = null;
      resume?.();
    },
    attach: (s, since) => session.attach(s, since),
    async submit(text: string): Promise<string | null> {
      // One call, not two: `run` *is* the submit. Posting `turn.submit` as
      // well would start the turn twice.
      let turn: string | null = null;
      for await (const frame of session.run(text)) {
        if (frame.type === "RUN_STARTED") turn = frame.runId;
        push(frame);
      }
      return turn;
    },
    cancel: (turn) => session.cancel(turn),
    answer: (prompt, answer) => session.answer(prompt, answer),
    intent: (surface, value) => session.intent(surface, value),
  };
}

/** Start. */
export async function main(argv = process.argv.slice(2), env = process.env): Promise<void> {
  checkNode();
  const endpoint = resolveEndpoint(argv, env);
  const id = resolveSession(argv, env);
  const session = AguiSession.connect(endpoint);
  try {
    await session.attach(id);
  } catch (error) {
    // `fetch failed` on its own tells a person nothing they can act on.
    throw new Error(
      `cannot reach the kernel at ${endpoint} (${
        error instanceof Error ? error.message : String(error)
      }). Is \`orrery serve\` still running?`,
    );
  }
  const connection = sessionConnection(session);
  const app = render(<App connection={connection} session={id} />, { exitOnCtrlC: false });
  await app.waitUntilExit();
}

/* c8 ignore start — the process wrapper, exercised by the smoke test. */
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main().catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`orrery ink: ${message}\n`);
    if (process.env["ORRERY_DEBUG"] && error instanceof Error) {
      process.stderr.write(`${error.stack ?? ""}\n`);
    }
    // 2 is "you configured it wrong"; 1 is "the kernel would not talk to me".
    process.exit(error instanceof Misconfigured ? 2 : 1);
  });
}
/* c8 ignore stop */
