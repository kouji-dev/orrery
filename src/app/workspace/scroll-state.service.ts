import { Injectable } from "@angular/core";
import type * as monacoApi from "monaco-editor";

const k = (agentId: string, path: string): string => `${agentId}:${path}`;

/**
 * Scroll positions for open file tabs, keyed `agentId:path` (same convention
 * as EditsStore). The pane destroys `app-file-view` when it switches to
 * terminal/diff/git and Monaco rebuilds per file, so positions must live
 * outside the components. Entries exist only while the file's tab is open —
 * tab close / rename / delete evicts them.
 */
@Injectable({ providedIn: "root" })
export class ScrollStateService {
  /** scrollTop of plain scroll bodies (markdown preview, media). */
  private readonly plain = new Map<string, number>();
  /** Monaco view states (scroll + cursor + folding). */
  private readonly view = new Map<string, monacoApi.editor.ICodeEditorViewState>();

  savePlain(agentId: string, path: string, scrollTop: number): void {
    this.plain.set(k(agentId, path), scrollTop);
  }

  getPlain(agentId: string, path: string): number | undefined {
    return this.plain.get(k(agentId, path));
  }

  saveView(agentId: string, path: string, state: monacoApi.editor.ICodeEditorViewState | null): void {
    if (state) this.view.set(k(agentId, path), state);
  }

  getView(agentId: string, path: string): monacoApi.editor.ICodeEditorViewState | undefined {
    return this.view.get(k(agentId, path));
  }

  clear(agentId: string, path: string): void {
    const key = k(agentId, path);
    this.plain.delete(key);
    this.view.delete(key);
  }

  clearAgent(agentId: string): void {
    const prefix = `${agentId}:`;
    for (const map of [this.plain, this.view]) {
      for (const key of [...map.keys()]) if (key.startsWith(prefix)) map.delete(key);
    }
  }
}
