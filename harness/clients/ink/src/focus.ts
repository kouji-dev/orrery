/**
 * Who has the keyboard.
 *
 * At most one thing does, and the order is not negotiable: policy first (a
 * consent bar is blocking something), then a surface that is asking, then the
 * composer. Two components listening to the same keypress is how a TUI gets a
 * reputation for eating input.
 */

import type { PromptView, StoreState, SurfaceView, TurnView } from "@orrery/client";

/** What has the keyboard. */
export type Focus = "consent" | "surface" | "composer";

/** The first prompt nobody has answered yet, anywhere in the transcript. */
export function pendingPrompt(state: StoreState): PromptView | null {
  for (const turn of state.turns) {
    for (const prompt of turn.prompts) {
      if (prompt.resolved === null) return prompt;
    }
  }
  return null;
}

/** The surface in a turn that is waiting for an answer, if any. */
export function askingSurface(turn: TurnView | null): SurfaceView | null {
  if (!turn) return null;
  for (const surface of turn.surfaces) {
    const asks = surface.kind.t === "question" || surface.kind.t === "form";
    if (asks && surface.status !== "done" && surface.status !== "cancelled") return surface;
  }
  return null;
}

/** Who gets the next keypress. */
export function focusOf(prompt: PromptView | null, live: TurnView | null): Focus {
  if (prompt) return "consent";
  if (askingSurface(live)) return "surface";
  return "composer";
}
