// Content-Length framing, the guest half.
//
// The same framing `orrery-jsonrpc`'s `framing::content_length` speaks, and the
// same one the hand-written fixture in `orrery-host-rpc/tests/fixtures/echo-ext`
// implements from the spec. Three implementations of one format is deliberate:
// the fixture proves the format is implementable without this file.

const CONTENT_LENGTH = /^content-length:\s*(\d+)\s*$/i;

/** Frame one message for the wire. */
export function encode(message) {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  return Buffer.concat([
    Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "ascii"),
    body,
  ]);
}

/**
 * Feed bytes in, get whole messages out.
 *
 * A split read is the normal case, not the edge one: a header can arrive
 * without its body, and a body can arrive with the next message's header stuck
 * to it.
 */
export class Decoder {
  #buffer = Buffer.alloc(0);

  /** @returns {object[]} every complete message these bytes finished. */
  push(chunk) {
    this.#buffer = Buffer.concat([this.#buffer, chunk]);
    const out = [];
    for (;;) {
      const separator = this.#buffer.indexOf("\r\n\r\n");
      if (separator === -1) return out;
      const headers = this.#buffer.subarray(0, separator).toString("utf8").split("\r\n");
      let length = null;
      for (const header of headers) {
        const match = CONTENT_LENGTH.exec(header);
        if (match) length = Number(match[1]);
      }
      if (length === null || Number.isNaN(length)) {
        // Unframeable. There is nothing useful to do with the rest of a stream
        // whose framing is gone.
        throw new Error("a message arrived with no usable Content-Length");
      }
      const start = separator + 4;
      if (this.#buffer.length < start + length) return out;
      const body = this.#buffer.subarray(start, start + length).toString("utf8");
      this.#buffer = this.#buffer.subarray(start + length);
      out.push(JSON.parse(body));
    }
  }
}
