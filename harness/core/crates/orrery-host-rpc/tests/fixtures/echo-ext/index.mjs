// A guest extension, hand-written against the protocol.
//
// Deliberately NOT built on @orrery/ext: this fixture is what proves the wire
// format is implementable from the spec alone, so it must not share code with
// the SDK. No network, no model, no dependencies.

const CONTENT_LENGTH = /^content-length:\s*(\d+)\s*$/i;

let buffer = Buffer.alloc(0);

process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const separator = buffer.indexOf("\r\n\r\n");
    if (separator === -1) return;
    const headers = buffer.subarray(0, separator).toString("utf8").split("\r\n");
    let length = null;
    for (const header of headers) {
      const match = CONTENT_LENGTH.exec(header);
      if (match) length = Number(match[1]);
    }
    if (length === null) {
      // Unreadable framing: there is nothing useful to do but stop.
      process.exit(2);
    }
    const start = separator + 4;
    if (buffer.length < start + length) return;
    const body = buffer.subarray(start, start + length).toString("utf8");
    buffer = buffer.subarray(start + length);
    handle(JSON.parse(body));
  }
});

function send(message) {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}

function handle(message) {
  if (message.method === undefined) return; // an answer to something we asked
  const { id, method, params } = message;

  if (method === "ext/load") {
    send({
      jsonrpc: "2.0",
      id,
      result: {
        tools: [
          { name: "say", description: "Say it back", atomic: false },
          { name: "boom", description: "Die mid-call", atomic: false },
          { name: "wait", description: "Never answer", atomic: false },
        ],
      },
    });
    return;
  }

  if (method === "tool/call") {
    if (params.tool === "boom") {
      // The case the host has to survive: the child goes away with a call in
      // flight and no reply on the wire.
      process.exit(3);
    }
    if (params.tool === "wait") {
      return; // answered by a cancellation, or by nothing
    }
    send({
      jsonrpc: "2.0",
      id,
      result: {
        outcome: {
          t: "ok",
          value: { said: params.input.text ?? null },
        },
      },
    });
    return;
  }

  if (method === "ext/shutdown") {
    send({ jsonrpc: "2.0", id, result: null });
    process.exit(0);
  }

  if (id !== undefined) {
    send({
      jsonrpc: "2.0",
      id,
      error: { code: -32601, message: `no such method: ${method}` },
    });
  }
}
