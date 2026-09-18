// The JSON-RPC client half: both directions on one connection.
//
// A guest answers `ext/load` and `tool/call` while its own `broker/*` requests
// are in flight up the same pipe, so this is a peer, not a client. The pending
// map is what keeps three concurrent broker calls from getting each other's
// replies.

import { Decoder, encode } from "./framing.mjs";

/** A refusal the broker sent back, as distinct from a bug. */
export class BrokerDenied extends Error {
  constructor(message, rule) {
    super(message);
    this.name = "BrokerDenied";
    /** Which rule refused, so a tool can say so rather than guessing. */
    this.rule = rule;
    this.denied = true;
  }
}

/** The call was stopped. */
export class Cancelled extends Error {
  constructor() {
    super("cancelled");
    this.name = "Cancelled";
  }
}

/** The code the host sends for a denial. */
const DENIED = -32001;

export class Peer {
  #input;
  #output;
  #decoder = new Decoder();
  #pending = new Map();
  #nextId = 1;
  #handlers;

  /**
   * @param handlers {{request: (method: string, params: any, id: any) => Promise<any>,
   *                   notify: (method: string, params: any) => void}}
   */
  constructor(input, output, handlers) {
    this.#input = input;
    this.#output = output;
    this.#handlers = handlers;
  }

  /** Start reading. Resolves when the other side goes away. */
  listen() {
    return new Promise((resolve) => {
      this.#input.on("data", (chunk) => {
        let messages;
        try {
          messages = this.#decoder.push(chunk);
        } catch (e) {
          // Framing is gone: nothing further on this stream means anything.
          process.stderr.write(`${e.message}\n`);
          process.exit(2);
        }
        for (const message of messages) this.#dispatch(message);
      });
      this.#input.on("end", resolve);
      this.#input.on("close", resolve);
    });
  }

  /** Ask the host something and wait for its answer. */
  request(method, params) {
    const id = this.#nextId++;
    const waiting = new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
    });
    this.#send({ jsonrpc: "2.0", id, method, params });
    return waiting;
  }

  /** Tell the host something. No answer. */
  notify(method, params) {
    this.#send({ jsonrpc: "2.0", method, params });
  }

  /** Answer a request the host made. */
  reply(id, result) {
    this.#send({ jsonrpc: "2.0", id, result });
  }

  /** Refuse a request the host made. */
  replyError(id, code, message) {
    this.#send({ jsonrpc: "2.0", id, error: { code, message } });
  }

  #send(message) {
    this.#output.write(encode(message));
  }

  #dispatch(message) {
    if (message.method === undefined) {
      const waiting = this.#pending.get(message.id);
      if (!waiting) return;
      this.#pending.delete(message.id);
      if (message.error) {
        const { code, message: text, data } = message.error;
        waiting.reject(
          code === DENIED
            ? new BrokerDenied(text, data?.rule)
            : Object.assign(new Error(text), { code }),
        );
      } else {
        waiting.resolve(message.result);
      }
      return;
    }
    if (message.id === undefined) {
      this.#handlers.notify(message.method, message.params ?? {});
      return;
    }
    // A request from the host. Answered asynchronously: a tool that takes a
    // second must not stop this peer answering anything else.
    Promise.resolve(this.#handlers.request(message.method, message.params ?? {}, message.id))
      .then((result) => {
        if (result !== undefined) this.reply(message.id, result);
      })
      .catch((e) => this.replyError(message.id, -32603, String(e?.message ?? e)));
  }
}
