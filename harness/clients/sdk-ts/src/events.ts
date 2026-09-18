/**
 * The AG-UI events this client understands, and the frame that carries our seq.
 *
 * The tags come from `@ag-ui/core`'s own `EventType`, not from a list retyped
 * here: that is what makes "an existing AG-UI client works unmodified" checkable
 * rather than asserted. The surface half comes from `@orrery/protocol`, which is
 * generated from `orrery-proto`. Nothing in this file is a hand-written frame
 * type.
 */

import { EventType } from "@ag-ui/core";
import type { Outcome, SurfaceKind } from "@orrery/protocol";

/** The `Custom` name a consent prompt rides under. */
export const CONSENT_REQUEST = "orrery.consent.request";
/** The `Custom` name a consent resolution rides under. */
export const CONSENT_RESOLVED = "orrery.consent.resolved";
/** The `Custom` name a turn's cancellation rides under. */
export const TURN_CANCELLED = "orrery.turn.cancelled";

/** One RFC 6902 op, plus the one RFC 6902 cannot express. */
export type PatchOp =
  | { op: "replace"; path: string; value: unknown }
  | { op: "add"; path: string; value: unknown }
  | { op: "remove"; path: string }
  /**
   * Ours. JSON Patch has no way to say "add these six characters to the end of
   * that string", and re-sending a markdown body once per token would undo the
   * whole point of the append.
   */
  | { op: "append"; path: string; value: string };

/** An AG-UI event, as `orrery-agui` encodes it. */
export type AguiEvent =
  | { type: typeof EventType.RUN_STARTED; threadId: string; runId: string }
  | {
      type: typeof EventType.RUN_FINISHED;
      threadId: string;
      runId: string;
      result?: unknown;
    }
  | { type: typeof EventType.RUN_ERROR; message: string; code?: string | null }
  | { type: typeof EventType.STEP_STARTED; stepName: string }
  | { type: typeof EventType.STEP_FINISHED; stepName: string }
  | {
      type: typeof EventType.TEXT_MESSAGE_START;
      messageId: string;
      role: string;
    }
  | {
      type: typeof EventType.TEXT_MESSAGE_CONTENT;
      messageId: string;
      delta: string;
    }
  | { type: typeof EventType.TEXT_MESSAGE_END; messageId: string }
  | {
      type: typeof EventType.TOOL_CALL_START;
      toolCallId: string;
      toolCallName: string;
      parentMessageId?: string;
    }
  | { type: typeof EventType.TOOL_CALL_ARGS; toolCallId: string; delta: string }
  | { type: typeof EventType.TOOL_CALL_END; toolCallId: string }
  | {
      type: typeof EventType.TOOL_CALL_RESULT;
      messageId: string;
      toolCallId: string;
      content: string;
      outcome?: Outcome;
    }
  | { type: typeof EventType.STATE_SNAPSHOT; snapshot: unknown }
  | { type: typeof EventType.STATE_DELTA; delta: PatchOp[] }
  | { type: typeof EventType.CUSTOM; name: string; value: Record<string, unknown> };

/**
 * An AG-UI event with our `seq` on it.
 *
 * `seq` is assigned once, per session, at the differ's output — never per
 * connection — so it is on the frame rather than implied by the order a socket
 * happened to deliver things in. `mergedFrom` is the first seq a coalesced
 * frame stands for, without which a merge and a lost frame look the same.
 */
export type Frame = AguiEvent & { seq: number; merged_from?: number };

/** The first sequence number a frame accounts for. */
export function firstSeq(frame: Frame): number {
  return frame.merged_from ?? frame.seq;
}

/** A surface kind we can still name after a patch has been applied to it. */
export type Kind = SurfaceKind;
