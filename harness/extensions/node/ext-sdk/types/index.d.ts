// Hand-written, because the package ships the JavaScript it was written in:
// there is no build step, and therefore nothing between the source a person
// reads and the code that runs.

/** The ceiling one call runs under. */
export interface ToolBudget {
  wall_clock_ms: number;
  output_bytes: number;
  memory_bytes?: number | null;
}

/** What a listing found. Never names what the call may not read. */
export interface Listing {
  entries: Array<{ path: string; is_dir: boolean; size?: number | null }>;
  truncated: boolean;
}

export interface ReadChunk {
  text: string;
  eof: boolean;
  total?: number | null;
}

export interface SpawnOutput {
  status: number | null;
  stdout: string;
  stderr: string;
  truncated: boolean;
}

export interface NetResponse {
  status: number;
  headers: Array<[string, string]>;
  body: string;
}

/** A described surface. Ratatui, Ink and `--json` each render it their own way. */
export interface Surface {
  kind: Record<string, unknown>;
}

export interface Ui {
  table(spec: { columns: string[]; rows: Array<Array<string | number>> }): Surface;
  text(value: string): Surface;
  markdown(value: string, complete?: boolean): Surface;
  readonly last: Surface | null;
}

/** Everything a tool call gets that is not its input. */
export interface Ctx {
  call: string;
  tool: string;
  budget: ToolBudget;
  /** Fires on `$/cancel`. One call, not the connection. */
  signal: AbortSignal;
  ui: Ui;
  fs: {
    list(path: string, opts?: { recursive?: boolean; limit?: number }): Promise<Listing>;
    read(path: string, opts?: { limit?: number; offset?: number }): Promise<ReadChunk>;
    write(path: string, text: string, opts?: { atomic?: boolean }): Promise<void>;
  };
  proc: {
    run(
      program: string,
      args?: string[],
      opts?: { cwd?: string; timeoutMs?: number },
    ): Promise<SpawnOutput>;
  };
  net: {
    fetch(
      url: string,
      opts?: { method?: string; headers?: Array<[string, string]>; body?: string },
    ): Promise<NetResponse>;
  };
  creds: { get(name: string): Promise<string> };
}

/** The aspects a tool cannot work without; an ungranted one disables it. */
export type Aspect =
  | "read"
  | "write"
  | "spawn"
  | "net"
  | "creds"
  | "ui"
  | "mem.read"
  | "mem.write";

export type Outcome =
  | { t: "ok"; value?: unknown; surface?: Surface }
  | { t: "denied"; rule: string; reason: string }
  | { t: "failed"; code: string; message: string }
  | { t: "cancelled"; reason: string }
  | { t: "truncated"; surface?: Surface; bytes_emitted: number; limit: number };

export interface ToolDefinition<I = Record<string, unknown>> {
  description?: string;
  /** JSON Schema. The registry validates the input against it before `run`. */
  input?: Record<string, unknown>;
  /** All-or-nothing: the broker reverts its effects if the call is cancelled. */
  atomic?: boolean;
  requires?: Aspect[];
  ceiling?: ToolBudget | null;
  run(input: I, ctx: Ctx): Promise<Outcome | Surface | unknown>;
}

export interface ExtensionDefinition {
  tools: Record<string, ToolDefinition<never>>;
  /** Anything that did not come up. A guest that says so degrades, not fails. */
  problems?: string[];
  /** `false` declares without serving — for a test that calls a tool directly. */
  serve?: boolean;
}

/** Declare an extension and serve it over stdio. */
export function defineExtension(definition: ExtensionDefinition): ExtensionDefinition;

/** Serve a definition on a pair of streams. Resolves when the host goes away. */
export function serve(
  definition: ExtensionDefinition,
  streams?: { input?: NodeJS.ReadableStream; output?: NodeJS.WritableStream },
): Promise<void>;

/** Build an outcome by hand when "it worked" is not the whole story. */
export const outcome: {
  ok(value?: unknown, surface?: Surface): Outcome;
  failed(code: string, message: unknown): Outcome;
  denied(rule: string, reason: string): Outcome;
  cancelled(reason?: string): Outcome;
  truncated(surface: Surface | undefined, bytesEmitted: number, limit: number): Outcome;
};

/** A refusal from the broker, as distinct from a bug. */
export class BrokerDenied extends Error {
  rule?: string;
  denied: true;
}

export class Cancelled extends Error {}
