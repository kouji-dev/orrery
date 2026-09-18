/**
 * `AguiSession`, the TypeScript half.
 *
 * The subscribe half is stock `@ag-ui/client`: `HttpAgent` runs the agent and
 * emits AG-UI events, exactly as it would against any AG-UI server. That *is*
 * the "existing AG-UI clients work unmodified" proof — if this file had to patch
 * the agent, the claim would be false.
 *
 * The control half is a small `fetch` client, because AG-UI's input path is a
 * run invocation and has no vocabulary for `session.attach(since)`,
 * `turn.cancel`, `intent` or `query`.
 */

import { HttpAgent } from "@ag-ui/client";

import type { Frame } from "./events.js";

/** One of the three ways to reach a kernel. */
export type Endpoint =
  | { kind: "inproc" }
  | { kind: "pipe"; name: string }
  | { kind: "http"; url: string; token?: string };

/**
 * Parse an endpoint string.
 *
 * The token rides the authority rather than a query parameter because a query
 * string ends up in logs, in shell history and in `ps` output, and a bearer
 * token is the whole of phase 1's auth story.
 */
export function parseEndpoint(raw: string): Endpoint {
  if (raw === "inproc:" || raw === "inproc") return { kind: "inproc" };
  if (raw.startsWith("pipe:")) {
    const name = raw.slice("pipe:".length);
    if (!name) throw new Error(`\`${raw}\` is not an endpoint`);
    return { kind: "pipe", name };
  }
  for (const scheme of ["http://", "https://"]) {
    if (!raw.startsWith(scheme)) continue;
    const rest = raw.slice(scheme.length);
    const at = rest.indexOf("@");
    const token = at > 0 ? rest.slice(0, at) : undefined;
    const host = (at >= 0 ? rest.slice(at + 1) : rest).replace(/\/+$/, "");
    if (!host) throw new Error(`\`${raw}\` is not an endpoint`);
    return { kind: "http", url: `${scheme}${host}`, token };
  }
  throw new Error(
    `\`${raw}\` is not an endpoint: expected \`inproc:\`, \`pipe:<name>\` or \`http://[token@]host:port\``,
  );
}

/** A control request, as `orrery-proto` spells it. */
export type Request = Record<string, unknown> & { t: string; id: string };

function reqId(): string {
  return crypto.randomUUID();
}

/** A connected session over HTTP + SSE. */
export class AguiSession {
  private readonly agent: HttpAgent;
  private lastSeq: number | null = null;

  private constructor(
    private readonly url: string,
    private readonly token: string | undefined,
    private session: string | null,
  ) {
    this.agent = new HttpAgent({
      url: `${url}/run`,
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
  }

  /** Connect to an endpoint. Only `http://` is reachable from TypeScript. */
  static connect(endpoint: Endpoint | string): AguiSession {
    const parsed = typeof endpoint === "string" ? parseEndpoint(endpoint) : endpoint;
    if (parsed.kind !== "http") {
      throw new Error(
        `${parsed.kind} is not reachable from TypeScript; use the HTTP listener`,
      );
    }
    return new AguiSession(parsed.url, parsed.token, null);
  }

  /** The stock `@ag-ui/client` agent, unmodified, for anyone who wants it. */
  httpAgent(): HttpAgent {
    return this.agent;
  }

  /** The last `seq` this session has seen. */
  seq(): number | null {
    return this.lastSeq;
  }

  /** Attach to a session, optionally replaying from where this client left off. */
  async attach(session: string, since?: number): Promise<void> {
    this.session = session;
    await this.control({
      t: "session.attach",
      id: reqId(),
      session,
      ...(since === undefined ? {} : { since }),
    });
  }

  /** Submit a turn. */
  async submit(text: string): Promise<string> {
    const answer = await this.control({
      t: "turn.submit",
      id: reqId(),
      session: this.requireSession(),
      input: { text },
    });
    const turn = (answer as { turn?: string }).turn;
    if (!turn) throw new Error(`no turn id in ${JSON.stringify(answer)}`);
    return turn;
  }

  /** Cancel a turn in flight. */
  async cancel(turn: string): Promise<void> {
    await this.control({
      t: "turn.cancel",
      id: reqId(),
      session: this.requireSession(),
      turn,
    });
  }

  /** Answer a consent prompt. */
  async answer(prompt: string, answer: string): Promise<void> {
    await this.control({ t: "consent.answer", id: reqId(), prompt, answer });
  }

  /**
   * Send what a surface produced.
   *
   * The whole of a client's write path. A client edit arrives here, as an intent
   * the kernel validates, never as a state mutation.
   */
  async intent(surface: string, value: unknown): Promise<void> {
    await this.control({
      t: "intent",
      id: reqId(),
      session: this.requireSession(),
      surface,
      value,
    });
  }

  /**
   * Start a run and yield its frames.
   *
   * `POST /run` with a `RunAgentInput`, parsed as SSE. `@ag-ui/client` would
   * give the same events with its own subscriber API; this form exists because
   * ours carry a `seq` the stock agent drops, and `seq` is the whole of the
   * resumption story.
   */
  async *run(text: string): AsyncGenerator<Frame> {
    const response = await fetch(`${this.url}/run`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({
        threadId: this.requireSession(),
        messages: [{ role: "user", content: text }],
      }),
    });
    if (!response.ok || !response.body) {
      throw new Error(`POST /run: ${response.status}`);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      buffer += decoder.decode(value, { stream: true });
      let at = buffer.indexOf("\n\n");
      while (at >= 0) {
        const block = buffer.slice(0, at);
        buffer = buffer.slice(at + 2);
        for (const line of block.split("\n")) {
          if (!line.startsWith("data: ")) continue;
          const frame = JSON.parse(line.slice("data: ".length)) as Frame;
          this.lastSeq = frame.seq;
          yield frame;
        }
        at = buffer.indexOf("\n\n");
      }
    }
  }

  private headers(): Record<string, string> {
    return {
      "content-type": "application/json",
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
    };
  }

  private requireSession(): string {
    if (!this.session) throw new Error("not attached to a session");
    return this.session;
  }

  private async control(request: Request): Promise<unknown> {
    const response = await fetch(`${this.url}/control`, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify(request),
    });
    if (!response.ok) throw new Error(`POST /control: ${response.status}`);
    return response.json();
  }
}
