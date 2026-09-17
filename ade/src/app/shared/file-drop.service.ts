import { inject, Injectable } from "@angular/core";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { quotePath } from "../utils";
import { DragService } from "./drag.service";

/**
 * Turns an OS file drop into inserted text. With `dragDropEnabled: true` in
 * tauri.conf.json the webview no longer sees dropped files through HTML5 dnd
 * (which never exposes absolute paths anyway); instead Tauri emits a drag-drop
 * event carrying the real absolute `paths`. This service listens for it and
 * drops those paths — space-separated and shell-quoted — where they landed:
 *  - over a text field (the spawn prompt, any `<textarea>`/text `<input>`) →
 *    inserted at the caret;
 *  - over an agent terminal → pasted into that agent's PTY;
 *  - otherwise → the terminal that currently holds the typing focus.
 *
 * One window-level listener for the app's lifetime; a no-op outside Tauri.
 */
@Injectable({ providedIn: "root" })
export class FileDropService {
  private readonly terminals = inject(TerminalService);
  private readonly drag = inject(DragService);
  private readonly agents = inject(AgentsStore);
  private started = false;
  private unlisten?: () => void;
  private unhookInternal?: () => void;

  /** Begin listening for OS file drops. Idempotent; silently no-ops when not
   *  running under Tauri (plain `ng serve` / unit tests). */
  start(): void {
    if (this.started) return;
    this.started = true;
    // Internal drags (B1.5): a file-tree row dragged onto a prompt/terminal.
    // Plain HTML5 dnd inside the window — works with or without Tauri.
    const over = (e: DragEvent): void => {
      if (this.drag.payload()?.kind === "file") e.preventDefault();
    };
    const drop = (e: DragEvent): void => {
      this.handleInternalDrop(e);
    };
    window.addEventListener("dragover", over);
    window.addEventListener("drop", drop);
    this.unhookInternal = () => {
      window.removeEventListener("dragover", over);
      window.removeEventListener("drop", drop);
    };
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          const paths = event.payload.paths ?? [];
          if (paths.length) this.route(paths, event.payload.position.x, event.payload.position.y);
        }),
      )
      .then((un) => {
        this.unlisten = un;
      })
      .catch(() => {
        /* not under Tauri — file drops just aren't wired */
      });
  }

  stop(): void {
    this.unlisten?.();
    this.unlisten = undefined;
    this.unhookInternal?.();
    this.unhookInternal = undefined;
    this.started = false;
  }

  /**
   * A file-tree row released over the app (B1.5): resolve the agent's absolute
   * path and insert it where the drop landed. Public for unit tests (jsdom
   * can't synthesize trusted DragEvents).
   */
  handleInternalDrop(e: {
    clientX: number;
    clientY: number;
    preventDefault(): void;
  }): void {
    const p = this.drag.payload();
    if (p?.kind !== "file" || !p.agentId || !p.relPath) return;
    e.preventDefault();
    const wt = this.agents.all().find((a) => a.id === p.agentId)?.worktree;
    this.drag.end();
    if (!wt) return;
    const sep = wt.includes("\\") ? "\\" : "/";
    const rel = sep === "\\" ? p.relPath.replace(/\//g, "\\") : p.relPath;
    this.routeAtCss([wt.replace(/[\\/]+$/, "") + sep + rel], e.clientX, e.clientY);
  }

  /**
   * Route dropped `paths` to the surface under the drop point. `physX/physY`
   * are physical pixels (Tauri's coordinate space) — converted to CSS pixels
   * for `elementFromPoint`. Public so the routing is unit-testable without the
   * Tauri event.
   */
  route(paths: string[], physX: number, physY: number): void {
    const dpr = window.devicePixelRatio || 1;
    this.routeAtCss(paths, physX / dpr, physY / dpr);
  }

  /** Same routing with CSS-pixel coordinates (internal drags, tests). */
  routeAtCss(paths: string[], cssX: number, cssY: number): void {
    if (!paths.length) return;
    const text = paths.map(quotePath).join(" ") + " ";
    const el = document.elementFromPoint(cssX, cssY) as HTMLElement | null;

    // 1) A real text field (spawn prompt / any textarea/text input) → insert at
    //    the caret and notify Angular via a synthetic input event.
    const field = el?.closest("textarea, input[type=text]") as
      | HTMLTextAreaElement
      | HTMLInputElement
      | null;
    if (field) {
      insertIntoField(field, text);
      return;
    }

    // 2) Over an agent terminal → paste into that agent's PTY. Not over one →
    //    fall back to whichever terminal currently has the typing focus.
    const host = el?.closest("[data-agent-id]") as HTMLElement | null;
    const id = host?.dataset["agentId"] ?? this.terminals.focusedAgentId();
    if (id) this.terminals.pasteToAgent(id, text);
  }
}

/** Insert `text` at the field's caret (replacing any selection) and fire an
 *  `input` event so framework bindings (Angular `(input)`) pick up the change. */
function insertIntoField(field: HTMLTextAreaElement | HTMLInputElement, text: string): void {
  const start = field.selectionStart ?? field.value.length;
  const end = field.selectionEnd ?? field.value.length;
  field.value = field.value.slice(0, start) + text + field.value.slice(end);
  const caret = start + text.length;
  try {
    field.setSelectionRange(caret, caret);
  } catch {
    /* some input types don't support selection ranges */
  }
  field.focus();
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
