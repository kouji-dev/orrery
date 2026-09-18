// An "existing service": hand-written against the protocol, sharing no code
// with the SDK, started through the manifest's [process] table rather than by
// the runtime's convention. Reads its greeting from [process.env], which is how
// a wrapped service is configured without touching its code.

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
    if (length === null) process.exit(2);
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
  if (message.method === undefined) return;
  const { id, method, params } = message;

  if (method === "ext/load") {
    send({
      jsonrpc: "2.0",
      id,
      result: { tools: [{ name: "ping", description: "Answer." }] },
    });
    return;
  }

  if (method === "tool/call") {
    send({
      jsonrpc: "2.0",
      id,
      result: {
        outcome: {
          t: "ok",
          value: {
            said: process.env.ORRERY_SERVICE_GREETING ?? "(unset)",
            tool: params.tool,
          },
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
    send({ jsonrpc: "2.0", id, error: { code: -32601, message: `no such method: ${method}` } });
  }
}
