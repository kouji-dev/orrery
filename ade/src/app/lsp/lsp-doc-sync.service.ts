import { effect, inject, Injectable, untracked } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { AgentsStore } from "../stores/agents.store";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { langId } from "../utils";
import { PaneNode } from "../workspace/pane-model";
import { isVirtualUri } from "../workspace/virtual-doc";
import { LspStatusStore } from "./lsp-status.store";

/** Keystrokes settle for this long before a didChange goes out. */
export const DOC_SYNC_DEBOUNCE_MS = 150;

/** One open file per pane leaf: the root id + worktree-relative path. */
export interface OpenFileKey {
  id: string;
  path: string;
}

/** Every file tab across every pane tree, deduped, virtual uris skipped. */
export function openFileKeys(roots: Record<string, PaneNode>): OpenFileKey[] {
  const seen = new Set<string>();
  const out: OpenFileKey[] = [];
  const walk = (n: PaneNode): void => {
    if (n.type === "leaf") {
      if (!n.agentId) return;
      for (const path of n.files ?? []) {
        if (isVirtualUri(path)) continue;
        const k = `${n.agentId}:${path}`;
        if (seen.has(k)) continue;
        seen.add(k);
        out.push({ id: n.agentId, path });
      }
      return;
    }
    walk(n.a);
    walk(n.b);
  };
  for (const root of Object.values(roots)) walk(root);
  return out;
}

interface Tracked {
  id: string;
  path: string;
  /** Text the server last received (didOpen / the last flushed didChange). */
  text: string;
  version: number;
}

/**
 * Document sync for language servers (M3): keeps a server's view of the
 * open buffers current. One effect over the open-file set (pane leaves'
 * `files[]`), the edit tick and the server list:
 *
 * - a key appears → `lsp_doc_open` with the EditsStore buffer (skipped until
 *   the buffer exists — the editor creates it on mount);
 * - the buffer changes → one `lsp_doc_change` per key after a 150 ms
 *   settle, full text, monotonic version;
 * - a key disappears → `lsp_doc_close`.
 *
 * Gated: a file is tracked while a starting/ready/idle server answers for
 * its language in the project of that root — OR while an installed + enabled
 * server pack claims the language and no instance exists there yet. That
 * first `lsp_doc_open` is what STARTS the server (the backend acquires one
 * per project on it and queues the document until the handshake is done);
 * with the `starting` row already tracked, its arrival re-opens nothing. A
 * parked instance (crashed, stopped, missing) gets no traffic: the file is
 * forgotten and re-opened when a syncable instance comes back. No pack at
 * all → no IPC ever.
 *
 * Started once from the shell (like InterestService) — `start()` only forces
 * construction; the work is the constructor effect.
 */
@Injectable({ providedIn: "root" })
export class LspDocSyncService {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly edits = inject(EditsStore);
  private readonly agents = inject(AgentsStore);
  private readonly lsp = inject(LspStatusStore);
  private readonly extensions = inject(ExtensionsStore);

  private readonly tracked = new Map<string, Tracked>();
  private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();

  constructor() {
    effect(() => {
      const keys = openFileKeys(this.ui.paneRoots());
      this.edits.editTick();
      this.lsp.servers();
      this.agents.all();
      this.extensions.servers();
      untracked(() => this.reconcile(keys));
    });
  }

  /** An installed + enabled server pack lists `lang`: opening a file of it
   *  is worth an IPC even before any instance exists (it starts one). */
  private packClaims(lang: string): boolean {
    return this.extensions.servers().some((p) => p.installed && p.enabled && p.languages.includes(lang));
  }

  /** Whether `lang` in `projectId` should receive document traffic now. */
  private wants(projectId: string, lang: string): boolean {
    if (!lang) return false;
    if (this.lsp.liveFor(projectId, lang)) return true;
    // a parked instance (crashed / stopped / missing) must not be poked
    // awake by an open — the backend's backoff and the user's Stop own that
    if (this.lsp.instanceFor(projectId, lang)) return false;
    return this.packClaims(lang);
  }

  start(): void {
    // construction is the work
  }

  /** The project a root belongs to: an agent's projectId, or the root id
   *  itself for a project tab (its pseudo-agent id IS the project id). */
  private projectOf(id: string): string {
    return this.agents.all().find((a) => a.id === id)?.projectId ?? id;
  }

  private reconcile(keys: OpenFileKey[]): void {
    const wanted = new Set<string>();
    for (const { id, path } of keys) {
      const k = `${id}:${path}`;
      const lang = langId(path);
      if (!this.wants(this.projectOf(id), lang)) {
        // no server for it (any more): forget silently — there is nothing
        // on the other side to close
        if (this.tracked.has(k)) this.forget(k);
        continue;
      }
      wanted.add(k);
      const buf = this.edits.get(id, path);
      if (!buf) continue; // the editor has not adopted the disk text yet
      const cur = this.tracked.get(k);
      if (!cur) {
        const rec: Tracked = { id, path, text: buf.text, version: 1 };
        this.tracked.set(k, rec);
        void this.bridge.invoke(Commands.LspDocOpen, { id, path, text: buf.text, languageId: lang }).catch(() => {});
        continue;
      }
      if (buf.text !== cur.text) this.schedule(k);
    }
    for (const k of [...this.tracked.keys()]) {
      if (wanted.has(k)) continue;
      const rec = this.tracked.get(k)!;
      this.forget(k);
      void this.bridge.invoke(Commands.LspDocClose, { id: rec.id, path: rec.path }).catch(() => {});
    }
  }

  private schedule(k: string): void {
    const t = this.timers.get(k);
    if (t) clearTimeout(t);
    this.timers.set(
      k,
      setTimeout(() => {
        this.timers.delete(k);
        this.flush(k);
      }, DOC_SYNC_DEBOUNCE_MS),
    );
  }

  private flush(k: string): void {
    const rec = this.tracked.get(k);
    if (!rec) return;
    const buf = this.edits.get(rec.id, rec.path);
    if (!buf || buf.text === rec.text) return;
    rec.text = buf.text;
    rec.version += 1;
    void this.bridge
      .invoke(Commands.LspDocChange, { id: rec.id, path: rec.path, text: rec.text, version: rec.version })
      .catch(() => {});
  }

  private forget(k: string): void {
    const t = this.timers.get(k);
    if (t) clearTimeout(t);
    this.timers.delete(k);
    this.tracked.delete(k);
  }
}
