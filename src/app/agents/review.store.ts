import { Injectable, signal } from "@angular/core";

export interface ReviewComment {
  id: string;
  file: string;
  view: "diff" | "file";
  lang: string;
  // 1-based line numbers in the CURRENT (new-side / working-tree) content —
  // deleted old-side lines are not commentable (they render as widgets, not
  // real document lines, in the unified merge view).
  fromLine: number;
  toLine: number;
  side: "new" | "file";
  snippet: string;
  lines: string[];
  note: string;
}

export interface ReviewPayloadItem {
  file: string;
  fromLine: number;
  toLine: number;
  snippet: string;
  note: string;
  block: boolean;
}

export interface ReviewPayload {
  comments: ReviewPayloadItem[];
  global: string;
}

export function isBlock(c: { fromLine: number; toLine: number }): boolean {
  return c.toLine > c.fromLine;
}

function refLines(c: { fromLine: number; toLine: number }): string {
  return c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`;
}

/** Pure: render the structured message the agent receives (no paste markers).
 *  General note leads; each comment is `file:line` (or `file:from-to`) + the note —
 *  no line content is transferred (the agent reads the file at those lines itself). */
export function assembleReviewMessage(p: ReviewPayload): string {
  const out: string[] = ["Review feedback:"];
  if (p.global) out.push(`[general] ${p.global}`);
  out.push("");
  p.comments.forEach((c) => {
    out.push(`${c.file}:${refLines(c)}`);
    out.push(`  → ${c.note}`);
  });
  return out.join("\n");
}

interface Slot {
  comments: ReviewComment[];
  seq: number;
}

@Injectable({ providedIn: "root" })
export class ReviewStore {
  // in-memory only — NO persistence. Signal map so views re-render on change.
  private readonly state = signal<Record<string, Slot>>({});

  list(agentId: string): ReviewComment[] {
    return this.state()[agentId]?.comments ?? [];
  }

  count(agentId: string): number {
    return this.list(agentId).length;
  }

  add(agentId: string, c: Omit<ReviewComment, "id">): string {
    let id = "";
    this.state.update((m) => {
      const slot = m[agentId] ?? { comments: [], seq: 0 };
      const seq = slot.seq + 1;
      id = `rc${seq}`;
      return { ...m, [agentId]: { seq, comments: [...slot.comments, { ...c, id }] } };
    });
    return id;
  }

  remove(agentId: string, id: string): void {
    this.state.update((m) => {
      const slot = m[agentId];
      if (!slot) return m;
      return { ...m, [agentId]: { ...slot, comments: slot.comments.filter((x) => x.id !== id) } };
    });
  }

  clear(agentId: string): void {
    this.state.update((m) => {
      if (!m[agentId]) return m;
      return { ...m, [agentId]: { ...m[agentId], comments: [] } };
    });
  }

  buildPayload(agentId: string, global: string): ReviewPayload {
    return {
      global: global.trim(),
      comments: this.list(agentId).map((c) => ({
        file: c.file,
        fromLine: c.fromLine,
        toLine: c.toLine,
        snippet: c.snippet,
        note: c.note,
        block: isBlock(c),
      })),
    };
  }
}
