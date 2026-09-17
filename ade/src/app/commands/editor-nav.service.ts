import { Injectable, signal } from "@angular/core";

export interface EditorNavTarget {
  agentId: string;
  /** Worktree-relative path of the file to position. */
  file: string;
  /** 1-based line; clamped to the document by the consumer. */
  line: number;
  /** 1-based column (optional). */
  col: number;
  /** Monotonic — retriggers even for the same location. */
  gen: number;
}

/**
 * Cross-component "go to line" channel (roadmap B2.3, Ctrl+L + find-in-files
 * click-through). Producers (go-to-line overlay, search results) post a target;
 * the mounted CodeMirror surface for that agent+file consumes it and scrolls.
 * A signal (not an event) so a target posted BEFORE the editor finishes its
 * async mount is still applied when the editor appears.
 */
@Injectable({ providedIn: "root" })
export class EditorNavService {
  private gen = 0;
  readonly target = signal<EditorNavTarget | null>(null);

  goTo(agentId: string, file: string, line: number, col = 1): void {
    this.target.set({ agentId, file, line, col, gen: ++this.gen });
  }

  /** Called by the consumer once the scroll was applied. */
  consume(t: EditorNavTarget): void {
    if (this.target() === t) this.target.set(null);
  }
}
