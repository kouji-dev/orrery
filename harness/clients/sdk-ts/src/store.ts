/**
 * `SurfaceStore`, the TypeScript half.
 *
 * The same projection as `clients/sdk-rs`, asserted against the same fixtures.
 * Text deltas become a markdown surface and tool calls become a tool stack
 * **here**, so no Ink component and no ratatui widget reimplements that — and
 * if these two files ever disagree, the conformance suite says so before a user
 * does.
 *
 * The store is a projection, never an authority. AG-UI's shared state is
 * bidirectional by design and ours is not: an edit a person makes travels back
 * as an `intent` the kernel validates.
 */

import { EventType } from "@ag-ui/core";
import type { Status, Surface, SurfaceKind } from "@orrery/protocol";

import {
  CONSENT_REQUEST,
  CONSENT_RESOLVED,
  TURN_CANCELLED,
  type Frame,
  type PatchOp,
  firstSeq,
} from "./events.js";

/** The turn surfaces land in when replay picks up mid-turn. */
export const DETACHED_TURN = "(detached)";

/** A hole in the sequence. */
export interface Gap {
  expected: number;
  got: number;
}

/** One surface, as a client holds it. */
export interface SurfaceView {
  id: string;
  status: Status | null;
  kind: SurfaceKind;
}

/** How a consent prompt was settled. */
export interface Resolution {
  answer: string;
  by: string;
}

/** A consent prompt, and how it stands. */
export interface PromptView {
  id: string;
  reason: string;
  deadline_ms: number;
  resolved: Resolution | null;
}

/** What went wrong in a turn. */
export interface TurnError {
  message: string;
  code: string | null;
}

/** One turn: everything the kernel did about one input. */
export interface TurnView {
  id: string;
  settled: boolean;
  cancelled: boolean;
  error: TurnError | null;
  /** In insertion order, not sorted. */
  surfaces: SurfaceView[];
  prompts: PromptView[];
}

/** The whole store, in the shape the conformance fixtures assert. */
export interface StoreState {
  last_seq: number | null;
  live: string | null;
  gaps: Gap[];
  turns: TurnView[];
}

/** What changed, so a renderer can redraw a part rather than everything. */
export type StoreChange =
  | { kind: "turn-started"; turn: string }
  | { kind: "turn-settled"; turn: string }
  | { kind: "surface-changed"; turn: string; surface: string }
  | { kind: "surface-removed"; turn: string; surface: string }
  | { kind: "prompt-raised"; prompt: string }
  | { kind: "prompt-resolved"; prompt: string }
  /** A renderer's cue to re-attach with `since = <the last seq it saw>`. */
  | { kind: "gap-detected"; expected: number; got: number }
  /** An event this client has no handler for. Ignored, not rejected. */
  | { kind: "ignored"; what: string };

function emptyState(): StoreState {
  return { last_seq: null, live: null, gaps: [], turns: [] };
}

function newTurn(id: string): TurnView {
  return {
    id,
    settled: false,
    cancelled: false,
    error: null,
    surfaces: [],
    prompts: [],
  };
}

/** RFC 6901 §4, in reverse. */
function unescape(token: string): string {
  return token.replace(/~1/g, "/").replace(/~0/g, "~");
}

/** Split `/surfaces/<id>/rest` into the id and the pointer below it. */
function splitPointer(path: string): { id: string; rest: string[] } | null {
  if (!path.startsWith("/surfaces/")) return null;
  const rest = path.slice("/surfaces/".length);
  const parts = rest.split("/");
  const id = unescape(parts[0] ?? "");
  if (!id) return null;
  return { id, rest: parts.slice(1).map(unescape) };
}

function outcomeStatus(outcome: unknown): Status {
  const t = (outcome as { t?: string } | undefined)?.t;
  if (t === "ok" || t === "truncated") return "done";
  if (t === "cancelled") return "cancelled";
  if (t === undefined) return "done";
  return "failed";
}

function outcomeSurface(outcome: unknown): Surface | undefined {
  const o = outcome as { surface?: Surface } | undefined;
  return o?.surface;
}

/** The projection itself. */
export class SurfaceStore {
  private data: StoreState = emptyState();

  /** The whole state, as the conformance fixtures spell it. */
  state(): StoreState {
    return this.data;
  }

  /** One turn by run id. */
  turn(id: string): TurnView | undefined {
    return this.data.turns.find((t) => t.id === id);
  }

  /** Every settled turn, oldest first. What scrollback shows. */
  settled(): TurnView[] {
    return this.data.turns.filter((t) => t.settled);
  }

  /** The turn in flight. What the live region shows. */
  live(): TurnView | undefined {
    const id = this.data.live;
    return id === null ? undefined : this.turn(id);
  }

  /** Every gap detected so far. */
  gaps(): Gap[] {
    return this.data.gaps;
  }

  /**
   * Apply one frame. Never rejects: an unknown event is reported as `ignored`
   * and still consumes its sequence number.
   */
  apply(frame: Frame): StoreChange[] {
    const changes = this.note(firstSeq(frame), frame.seq);
    return changes.concat(this.applyEvent(frame));
  }

  /**
   * Account for a frame this build could not even parse.
   *
   * An event type from a newer kernel still occupies a sequence number, so
   * skipping it silently would turn every forward-compatible addition into a
   * phantom gap.
   */
  applyUnknown(seq: number, what: string): StoreChange[] {
    const changes = this.note(seq, seq);
    changes.push({ kind: "ignored", what });
    return changes;
  }

  private note(first: number, seq: number): StoreChange[] {
    const changes: StoreChange[] = [];
    const prev = this.data.last_seq;
    if (prev !== null && first !== prev + 1) {
      const gap = { expected: prev + 1, got: first };
      this.data.gaps.push(gap);
      changes.push({ kind: "gap-detected", ...gap });
    }
    this.data.last_seq = seq;
    return changes;
  }

  private applyEvent(event: Frame): StoreChange[] {
    switch (event.type) {
      case EventType.RUN_STARTED: {
        this.data.turns.push(newTurn(event.runId));
        this.data.live = event.runId;
        return [{ kind: "turn-started", turn: event.runId }];
      }
      case EventType.RUN_FINISHED:
        return this.settle(null);
      case EventType.RUN_ERROR:
        return this.settle({
          message: event.message,
          code: event.code ?? null,
        });
      case EventType.TEXT_MESSAGE_START:
        return this.upsert(event.messageId, (s) => {
          s.status = "running";
          s.kind = { t: "markdown", value: "", complete: false };
        });
      case EventType.TEXT_MESSAGE_CONTENT:
        return this.upsert(event.messageId, (s) => {
          if (s.kind.t === "markdown") {
            s.kind.value += event.delta;
          } else {
            s.kind = { t: "markdown", value: event.delta, complete: false };
            s.status = "running";
          }
        });
      case EventType.TEXT_MESSAGE_END:
        return this.upsert(event.messageId, (s) => {
          if (s.kind.t === "markdown") s.kind.complete = true;
          s.status = "done";
        });
      case EventType.TOOL_CALL_START:
        return this.upsert(event.toolCallId, (s) => {
          s.status = "running";
          s.kind = {
            t: "stack",
            dir: "column",
            title: event.toolCallName,
            collapsed: false,
            children: [{ kind: { t: "text", value: "", style: "code" } }],
          };
        });
      case EventType.TOOL_CALL_ARGS:
        return this.upsert(event.toolCallId, (s) => {
          if (s.kind.t !== "stack") return;
          const first = s.kind.children?.[0];
          if (first && first.kind.t === "text") first.kind.value += event.delta;
        });
      // The arguments are complete; the call is not. Nothing to draw yet.
      case EventType.TOOL_CALL_END:
        return [{ kind: "ignored", what: `TOOL_CALL_END:${event.toolCallId}` }];
      case EventType.TOOL_CALL_RESULT: {
        const status = outcomeStatus(event.outcome);
        const result: Surface = outcomeSurface(event.outcome) ?? {
          kind: { t: "text", value: event.content },
        };
        return this.upsert(event.toolCallId, (s) => {
          s.status = status;
          if (s.kind.t === "stack") {
            s.kind.children = [...(s.kind.children ?? []), result];
          }
        });
      }
      case EventType.STATE_SNAPSHOT:
        return this.snapshot(event.snapshot);
      case EventType.STATE_DELTA:
        return event.delta.flatMap((op) => this.patch(op));
      case EventType.CUSTOM:
        return this.custom(event.name, event.value);
      default:
        return [{ kind: "ignored", what: (event as { type: string }).type }];
    }
  }

  /** The turn new surfaces land in. */
  private target(): TurnView {
    if (this.data.live !== null) {
      const live = this.turn(this.data.live);
      if (live) return live;
    }
    for (let i = this.data.turns.length - 1; i >= 0; i -= 1) {
      const turn = this.data.turns[i]!;
      if (!turn.settled) return turn;
    }
    const detached = newTurn(DETACHED_TURN);
    this.data.turns.push(detached);
    return detached;
  }

  private upsert(id: string, edit: (s: SurfaceView) => void): StoreChange[] {
    const turn = this.target();
    let surface = turn.surfaces.find((s) => s.id === id);
    if (!surface) {
      surface = { id, status: null, kind: { t: "text", value: "" } };
      turn.surfaces.push(surface);
    }
    edit(surface);
    return [{ kind: "surface-changed", turn: turn.id, surface: id }];
  }

  private settle(error: TurnError | null): StoreChange[] {
    const turn = this.target();
    turn.settled = true;
    turn.error = error;
    // A turn that stopped closes its message as cancelled, never as done: a
    // half-answer must not read as an answer.
    const closing: Status = turn.cancelled ? "cancelled" : "done";
    for (const surface of turn.surfaces) {
      if (surface.status === "running") {
        if (surface.kind.t === "markdown") surface.kind.complete = true;
        surface.status = closing;
      }
    }
    this.data.live = null;
    return [{ kind: "turn-settled", turn: turn.id }];
  }

  private snapshot(snapshot: unknown): StoreChange[] {
    const surfaces = (snapshot as { surfaces?: Record<string, unknown> } | null)
      ?.surfaces;
    if (!surfaces) return [{ kind: "ignored", what: "STATE_SNAPSHOT" }];
    const turn = this.target();
    turn.surfaces = [];
    const changes: StoreChange[] = [];
    for (const [id, value] of Object.entries(surfaces)) {
      turn.surfaces.push(surfaceFromJson(id, value));
      changes.push({ kind: "surface-changed", turn: turn.id, surface: id });
    }
    return changes;
  }

  private patch(op: PatchOp): StoreChange[] {
    const split = splitPointer(op.path);
    if (!split) {
      return [{ kind: "ignored", what: `patch outside /surfaces: ${op.path}` }];
    }
    const { id, rest } = split;
    const turn = this.target();

    if (rest.length === 0) {
      if (op.op === "remove") {
        turn.surfaces = turn.surfaces.filter((s) => s.id !== id);
        return [{ kind: "surface-removed", turn: turn.id, surface: id }];
      }
      if (op.op === "append") {
        return [{ kind: "ignored", what: "append to a whole surface" }];
      }
      // A `replace` at the surface root creates as well as swaps: the kernel
      // sends one op whether or not this client has seen the surface before,
      // because it does not track what each client knows.
      const view = surfaceFromJson(id, op.value);
      const at = turn.surfaces.findIndex((s) => s.id === id);
      if (at >= 0) turn.surfaces[at] = view;
      else turn.surfaces.push(view);
      return [{ kind: "surface-changed", turn: turn.id, surface: id }];
    }

    const surface = turn.surfaces.find((s) => s.id === id);
    if (!surface) {
      return [{ kind: "ignored", what: `patch to an unknown surface ${id}` }];
    }
    let slot: Record<string, unknown> = surface as unknown as Record<string, unknown>;
    for (const segment of rest.slice(0, -1)) {
      const next = slot[segment];
      if (next === null || typeof next !== "object") {
        return [{ kind: "ignored", what: `no such field ${op.path}` }];
      }
      slot = next as Record<string, unknown>;
    }
    const leaf = rest[rest.length - 1]!;
    if (!(leaf in slot)) {
      return [{ kind: "ignored", what: `no such field ${op.path}` }];
    }
    if (op.op === "append") {
      const current = slot[leaf];
      slot[leaf] = typeof current === "string" ? current + op.value : op.value;
    } else if (op.op === "remove") {
      slot[leaf] = null;
    } else {
      slot[leaf] = op.value;
    }
    return [{ kind: "surface-changed", turn: turn.id, surface: id }];
  }

  private custom(name: string, value: Record<string, unknown>): StoreChange[] {
    if (name === CONSENT_REQUEST) {
      const prompt = promptFromJson(value);
      if (!prompt) return [{ kind: "ignored", what: name }];
      this.target().prompts.push(prompt);
      return [{ kind: "prompt-raised", prompt: prompt.id }];
    }
    if (name === CONSENT_RESOLVED) {
      const id = String(value["prompt_id"] ?? "");
      const resolution: Resolution = {
        answer: String(value["answer"] ?? "deny"),
        by: String(value["by"] ?? "fallback"),
      };
      const existing = this.data.turns
        .flatMap((t) => t.prompts)
        .find((p) => p.id === id);
      if (existing) {
        existing.resolved = resolution;
      } else {
        // Replayed as already-resolved: this client never saw the question,
        // only the answer.
        const prompt = promptFromJson(value) ?? {
          id,
          reason: "",
          deadline_ms: 0,
          resolved: null,
        };
        prompt.resolved = resolution;
        this.target().prompts.push(prompt);
      }
      return [{ kind: "prompt-resolved", prompt: id }];
    }
    if (name === TURN_CANCELLED) {
      const turn = this.target();
      turn.cancelled = true;
      return [{ kind: "turn-settled", turn: turn.id }];
    }
    return [{ kind: "ignored", what: name }];
  }
}

function surfaceFromJson(id: string, value: unknown): SurfaceView {
  const raw = (value ?? {}) as { status?: Status | null; kind?: SurfaceKind };
  return {
    id,
    status: raw.status ?? null,
    kind: structuredClone(raw.kind ?? { t: "text", value: "" }),
  };
}

function promptFromJson(value: Record<string, unknown>): PromptView | null {
  const prompt = value["prompt"] as Record<string, unknown> | undefined;
  const id = prompt?.["id"];
  if (typeof id !== "string") return null;
  return {
    id,
    reason: typeof prompt?.["reason"] === "string" ? (prompt["reason"] as string) : "",
    deadline_ms:
      typeof value["deadline_ms"] === "number" ? (value["deadline_ms"] as number) : 0,
    resolved: null,
  };
}
