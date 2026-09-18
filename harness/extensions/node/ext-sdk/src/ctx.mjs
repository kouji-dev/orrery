// What a tool is given: `ctx.fs`, `ctx.proc`, `ctx.net`, `ctx.creds`, `ctx.ui`.
//
// Every one of them is a request travelling up the connection the `tool/call`
// came down. Nothing here is a handle: there is no `open`, no `Child`, no
// socket, because a handle once given cannot be budgeted, cancelled or
// audited afterwards.

/** Host→guest and guest→host method names. Kept in one place on purpose. */
export const METHOD = {
  load: "ext/load",
  call: "tool/call",
  shutdown: "ext/shutdown",
  cancel: "$/cancel",
  list: "broker/list",
  read: "broker/read",
  write: "broker/write",
  spawn: "broker/spawn",
  fetch: "broker/fetch",
  credential: "broker/credential",
};

/** Describes a surface; never draws one. */
class Ui {
  #surfaces = [];

  /** Rows under headers. */
  table({ columns, rows }) {
    return this.#push({
      t: "table",
      columns,
      rows: rows.map((row) => row.map((cell) => ({ text: String(cell) }))),
    });
  }

  /** A run of text. */
  text(value) {
    return this.#push({ t: "text", value: String(value) });
  }

  /** Markdown. `complete` is false while it is still streaming. */
  markdown(value, complete = true) {
    return this.#push({ t: "markdown", value: String(value), complete });
  }

  /** The last surface described, which is what the outcome carries. */
  get last() {
    return this.#surfaces.at(-1) ?? null;
  }

  #push(kind) {
    const surface = { kind };
    this.#surfaces.push(surface);
    return surface;
  }
}

/**
 * The context for one call.
 *
 * `signal` is an `AbortSignal` that fires on `$/cancel` — `turn.cancel` reaches
 * one call, not the connection.
 */
export function makeCtx(peer, { call, tool, budget, signal }) {
  const guard = () => {
    if (signal.aborted) throw new DOMException("cancelled", "AbortError");
  };

  return {
    call,
    tool,
    budget,
    signal,
    ui: new Ui(),
    fs: {
      /** What is under a directory. What this call may not read is not named. */
      async list(path, { recursive = false, limit = 20000 } = {}) {
        guard();
        return peer.request(METHOD.list, { path, recursive, limit });
      },
      /** At most `limit` bytes. There is no "read the whole file". */
      async read(path, { limit = budget.output_bytes, offset = 0 } = {}) {
        guard();
        return peer.request(METHOD.read, { path, offset, limit });
      },
      /** Replace a file's contents, all-or-nothing unless told otherwise. */
      async write(path, text, { atomic = true } = {}) {
        guard();
        await peer.request(METHOD.write, { path, text, atomic });
      },
    },
    proc: {
      /** Run a program under the call's ceiling and containment. */
      async run(program, args = [], { cwd, timeoutMs } = {}) {
        guard();
        return peer.request(METHOD.spawn, {
          program,
          args,
          cwd: cwd ?? null,
          timeout_ms: timeoutMs ?? null,
        });
      },
    },
    net: {
      /** One HTTP request, policy-checked like everything else. */
      async fetch(url, { method = "GET", headers = [], body } = {}) {
        guard();
        return peer.request(METHOD.fetch, { method, url, headers, body: body ?? null });
      },
    },
    creds: {
      /** A named credential. The value never reaches the transcript. */
      async get(name) {
        guard();
        const reply = await peer.request(METHOD.credential, { name });
        return reply.value;
      },
    },
  };
}

/** The outcomes a tool may return, in the one shape every client renders. */
export const outcome = {
  ok: (value, surface) => ({ t: "ok", value: value ?? null, surface: surface ?? undefined }),
  failed: (code, message) => ({ t: "failed", code, message: String(message) }),
  denied: (rule, reason) => ({ t: "denied", rule, reason }),
  cancelled: (reason = "user") => ({ t: "cancelled", reason }),
  truncated: (surface, bytesEmitted, limit) => ({
    t: "truncated",
    surface,
    bytes_emitted: bytesEmitted,
    limit,
  }),
};
