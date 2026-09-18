/**
 * Task 9: the launch contract, end to end over a real socket.
 *
 * The plan asks for `orrery serve --provider fixture:…` here. That command is
 * plan 17's and still exits 2 ("not implemented"), so this test stands a
 * loopback HTTP listener up in-process that speaks the two endpoints
 * `@orrery/client` uses — `POST /control` and `POST /run` as SSE — and replays
 * a conformance fixture down it. Nothing is mocked on this side of the socket:
 * the client under test is `AguiSession` + `sessionConnection` + `<App>`, the
 * same path `orrery --ui ink` takes. When `orrery serve` lands, the only line
 * that changes is the one that starts the server.
 *
 * No model, no network: `127.0.0.1`, a file of recorded frames.
 */

import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

import { render } from "ink-testing-library";
import { afterEach, describe, expect, it } from "vitest";

import { AguiSession, type Frame } from "@orrery/client";

import { App } from "../src/app.js";
import { sessionConnection } from "../src/index.js";
import { frames, offset, settle } from "./fake.js";

let server: Server | null = null;

afterEach(async () => {
  await new Promise<void>((resolve) => {
    server?.closeAllConnections?.();
    server?.close(() => resolve());
    if (!server) resolve();
  });
  server = null;
});

/** A kernel-shaped listener that replays recorded frames. */
async function serve(list: Frame[]): Promise<string> {
  server = createServer((request, response) => {
    if (request.url?.startsWith("/control")) {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ t: "ok", turn: "turn-2" }));
      return;
    }
    response.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
    });
    for (const frame of list) response.write(`data: ${JSON.stringify(frame)}\n\n`);
    response.end();
  });
  await new Promise<void>((resolve) => server!.listen(0, "127.0.0.1", resolve));
  const { port } = server!.address() as AddressInfo;
  return `http://127.0.0.1:${port}`;
}

describe("a standalone run against a listening kernel", () => {
  it("smoke.ink_renders_a_fixture_turn", async () => {
    // One tool call, then the table that tool returned.
    const tool = frames("tool-call");
    const table = offset(
      frames("table-then-resort").filter((f) => f.type === "STATE_DELTA"),
      tool.length,
    );
    const endpoint = await serve([...tool.slice(0, -1), ...table, tool[tool.length - 1]!]);

    // Exactly what `orrery --ui ink` does with `$ORRERY_ENDPOINT`.
    const session = AguiSession.connect(endpoint);
    await session.attach("sess-1");
    const connection = sessionConnection(session);
    const ui = render(<App connection={connection} session="sess-1" width={72} />);

    await settle(4);
    for (const ch of "list the tools") ui.stdin.write(ch);
    ui.stdin.write("\r");
    await settle(40);

    const frame = ui.frames.join("\n");
    // The tool call, its arguments and the table it returned.
    expect(frame).toContain("builtin.read");
    expect(frame).toContain("crate");
    expect(frame).toContain("orrery-proto");
    expect(frame).toContain("2358");

    connection.close();
    ui.unmount();
  });

  it("says something useful when the endpoint is not listening", async () => {
    const session = AguiSession.connect("http://127.0.0.1:1");
    await expect(session.attach("sess-1")).rejects.toThrow();
  });
});
