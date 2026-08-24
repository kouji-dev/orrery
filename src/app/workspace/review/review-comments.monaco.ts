import type * as monacoApi from "monaco-editor";

import type { MonacoApi } from "../monaco-loader";

/** The slice of a ReviewComment the editor needs to render. */
export interface ReviewCommentLite {
  id: string;
  fromLine: number; // 1-based, inclusive
  toLine: number; // 1-based, inclusive
  note: string;
}

export interface CommentHost {
  /** Persist a new comment on [fromLine, toLine] (1-based, new-side). */
  save(fromLine: number, toLine: number, note: string): void;
  /** Delete a saved comment. */
  remove(id: string): void;
}

/** Drop comments that no longer fit the document; clamp partial overhangs. */
export function clampComments(comments: ReviewCommentLite[], docLines: number): ReviewCommentLite[] {
  return comments
    .filter((c) => c.fromLine >= 1 && c.fromLine <= docLines)
    .map((c) => (c.toLine <= docLines ? c : { ...c, toLine: docLines }));
}

/**
 * Monaco port of the inline-review comment UX (review-comments.ext.ts): hover
 * "+" in the glyph margin, drag range select, inline composer and saved-comment
 * cards as view zones. Same host contract and the same `rc-*` class names, so
 * the visual language carries over 1:1.
 *
 * Mapping from the CM extension:
 *  - line tints (`rc-covered`/`rc-selected`) → whole-line decorations
 *  - gutter markers (+ / anchor / dot)       → glyph-margin decoration classes
 *    (Monaco's glyph margin takes class names, not DOM — the icons are CSS)
 *  - card / composer block widgets           → view zones (measured after
 *    mount, then re-laid-out, since zones need an explicit pixel height)
 */

export interface MonacoReviewApi {
  /** Push the current comment list (idempotent). */
  setComments(comments: ReviewCommentLite[]): void;
  /** Open the inline composer on a line range (drag-release target). */
  openComposer(fromLine: number, toLine: number): void;
  dispose(): void;
}

// ----- tiny inline icons (lucide paths — same set as the CM extension) -----
function svgIcon(paths: string[], px: number): SVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(px));
  svg.setAttribute("height", String(px));
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2.4");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  for (const d of paths) {
    const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
    p.setAttribute("d", d);
    svg.appendChild(p);
  }
  return svg;
}
const CHAT = ["M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"];
const TRASH = ["M3 6h18", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2", "M10 11v6", "M14 11v6"];

// ----- pure DOM factories (exported for the spec) -----

/** Saved-comment card — identical structure/classes to the CM widget. */
export function buildCard(c: ReviewCommentLite, onDelete: (id: string) => void): HTMLElement {
  const root = document.createElement("div");
  root.className = "rc-card";
  const bar = document.createElement("span");
  bar.className = "rc-card-bar";
  root.appendChild(bar);
  const body = document.createElement("div");
  body.className = "rc-card-body";
  root.appendChild(body);

  const head = document.createElement("div");
  head.className = "rc-card-head";
  const avatar = document.createElement("span");
  avatar.className = "rc-card-avatar";
  avatar.textContent = "YOU";
  const who = document.createElement("span");
  who.className = "rc-card-who";
  who.textContent = "You";
  const ref = document.createElement("span");
  ref.className = "rc-card-ref tnum";
  ref.textContent = "· " + (c.toLine > c.fromLine ? `lines ${c.fromLine}-${c.toLine}` : `line ${c.fromLine}`);
  const chip = document.createElement("span");
  chip.className = "rc-card-chip";
  chip.textContent = "pending";
  const del = document.createElement("button");
  del.className = "rc-card-del";
  del.title = "Delete comment";
  del.appendChild(svgIcon(TRASH, 12));
  del.addEventListener("click", () => onDelete(c.id));
  head.append(avatar, who, ref, chip, del);
  body.appendChild(head);

  const note = document.createElement("div");
  note.className = "rc-card-note";
  note.textContent = c.note;
  body.appendChild(note);
  return root;
}

/** Inline composer — identical structure/classes to the CM widget. */
export function buildComposer(
  from: number,
  to: number,
  cb: { save(note: string): void; cancel(): void },
): HTMLElement {
  const root = document.createElement("div");
  root.className = "rc-composer";
  const bar = document.createElement("span");
  bar.className = "rc-composer-bar";
  root.appendChild(bar);
  const body = document.createElement("div");
  body.className = "rc-composer-body";
  root.appendChild(body);

  const head = document.createElement("div");
  head.className = "rc-composer-head";
  const icon = svgIcon(CHAT, 12);
  icon.classList.add("rc-composer-icon");
  const label = document.createElement("span");
  label.className = "rc-composer-label";
  const range = to > from ? `lines ${from}–${to}` : `line ${from}`;
  label.innerHTML = `Comment on <b>${range}</b>`;
  const tag = document.createElement("span");
  tag.className = "rc-composer-tag up";
  tag.textContent = "review · queued for agent";
  head.append(icon, label, tag);
  body.appendChild(head);

  const ta = document.createElement("textarea");
  ta.rows = 3;
  ta.placeholder = "Leave a note for the agent — what to change and why…";
  ta.className = "rc-composer-ta";
  body.appendChild(ta);

  const foot = document.createElement("div");
  foot.className = "rc-composer-foot";
  const hint = document.createElement("span");
  hint.className = "rc-composer-hint";
  hint.innerHTML = `<span class="kbd">⌘</span> <span class="kbd">↵</span> to save`;
  const spacer = document.createElement("span");
  spacer.style.marginLeft = "auto";
  // This widget lives OUTSIDE Angular (Monaco owns the DOM), so it cannot use
  // <kj-button>. kouji's button skin is pure class + data-attribute CSS, so the
  // native elements opt into it directly — same control ladder as the app,
  // no second button recipe. (The old .btn/.primary/.ghost-hair classes died
  // with the migration and left these two unstyled.)
  const cancel = document.createElement("button");
  cancel.className = "kj-button rc-composer-btn";
  cancel.dataset["variant"] = "outline";
  cancel.dataset["size"] = "xs";
  cancel.textContent = "Cancel";
  const save = document.createElement("button");
  save.className = "kj-button rc-composer-btn";
  save.dataset["variant"] = "default";
  save.dataset["size"] = "xs";
  save.textContent = "Save";
  save.disabled = true;
  foot.append(hint, spacer, cancel, save);
  body.appendChild(foot);

  const doSave = (): void => {
    const note = ta.value.trim();
    if (!note) return;
    cb.save(note);
  };
  ta.addEventListener("input", () => (save.disabled = !ta.value.trim()));
  ta.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") doSave();
    if (e.key === "Escape") cb.cancel();
  });
  cancel.addEventListener("click", () => cb.cancel());
  save.addEventListener("click", doSave);
  setTimeout(() => ta.focus(), 0);
  return root;
}

// ----- global styles (Monaco decorations take class names — inject once) -----

const STYLE_ID = "rc-monaco-styles";
function ensureStyles(): void {
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = `
.monaco-editor .rc-covered { box-shadow: inset 2px 0 0 var(--ui-ind); }
.monaco-editor .rc-selected { background: var(--ui-sel); }
.monaco-editor .rc-glyph-plus {
  cursor: grab; border-radius: 4px; background: var(--ui-fill);
  -webkit-mask: url('data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2.4" stroke-linecap="round"><path d="M5 12h14"/><path d="M12 5v14"/></svg>') center / 11px 11px no-repeat, linear-gradient(#000 0 0);
  -webkit-mask-composite: xor; mask-composite: exclude;
  transform: scale(0.78);
}
.monaco-editor .rc-glyph-anchor {
  border-radius: 4px; background: var(--ui-fill);
  -webkit-mask: url('data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2.4" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>') center / 10px 10px no-repeat, linear-gradient(#000 0 0);
  -webkit-mask-composite: xor; mask-composite: exclude;
  transform: scale(0.72);
}
.monaco-editor .rc-glyph-dot {
  background: radial-gradient(circle at center, var(--ui-line) 0 2.5px, transparent 3px);
}
/* Monaco's view-zone layer is display-only by default — the comment cards and
   composer are interactive, so their zones must accept pointer events and
   stack above the text layers. */
.monaco-editor .rc-zone {
  font-family: var(--font-sans, inherit);
  pointer-events: auto;
  position: relative;
  z-index: 10;
}
.rc-card { display: flex; background: var(--panel); border-top: 1px solid var(--hair); border-bottom: 1px solid var(--hair); }
.rc-card-bar { width: 24px; flex: none; background: var(--ui-sel); box-shadow: inset 2px 0 0 var(--ui-ind); }
.rc-card-body { flex: 1; padding: 9px 14px 9px 12px; min-width: 0; }
.rc-card-head { display: flex; align-items: center; gap: 7px; }
.rc-card-avatar { width: 17px; height: 17px; flex: none; border-radius: 50%; display: grid; place-items: center; font-size: var(--fs-micro); font-weight: var(--fw-strong); color: var(--ui-ink); background: var(--ui-sel); border: 1px solid var(--ui-sel-2); }
.rc-card-who { font-size: var(--fs-badge); color: var(--ink-2); }
.rc-card-ref { font-size: var(--fs-micro); color: var(--ink-4); }
.rc-card-chip { font-size: var(--fs-micro); padding: 0 5px; color: var(--ui-ink); border: 1px solid var(--ui-sel-2); border-radius: 999px; }
.rc-card-del { margin-left: auto; background: transparent; border: none; color: var(--ink-4); cursor: pointer; display: flex; padding: 3px; border-radius: 3px; }
.rc-card-del:hover { color: var(--code-del-ink); }
.rc-card-note { font-size: var(--fs-badge); color: var(--ink); line-height: 1.5; margin-top: 5px; white-space: pre-wrap; word-break: break-word; }
.rc-composer { display: flex; background: var(--ui-sel); border-top: 1px solid var(--ui-sel-2); border-bottom: 1px solid var(--hair); }
.rc-composer-bar { width: 24px; flex: none; background: var(--ui-sel-2); }
.rc-composer-body { flex: 1; padding: 10px 14px 10px 12px; }
.rc-composer-head { display: flex; align-items: center; gap: 7px; margin-bottom: 7px; }
.rc-composer-icon { color: var(--ui-ink); }
.rc-composer-label { font-size: var(--fs-micro); color: var(--ink-2); }
.rc-composer-label b { color: var(--ink); }
.rc-composer-tag { margin-left: auto; font-size: var(--fs-micro); color: var(--ink-4); }
.rc-composer-ta { width: 100%; resize: vertical; min-height: 56px; background: var(--panel-2); border: 1px solid var(--hair); border-radius: var(--r-sm); padding: 8px 10px; color: var(--ink); font-family: var(--font-mono); font-size: var(--fs-badge); line-height: 1.55; outline: none; }
.rc-composer-foot { display: flex; align-items: center; gap: 8px; margin-top: 8px; }
.rc-composer-hint { font-size: var(--fs-micro); color: var(--ink-4); }
/* geometry comes from .kj-button[data-size="xs"]; nothing to add */
`;
  document.head.appendChild(style);
}

// -------------------------------------------------------------- controller ----

interface DragState {
  anchor: number;
  current: number;
}
interface ComposerState {
  from: number;
  to: number;
}

export function attachReviewComments(
  monaco: MonacoApi,
  editor: monacoApi.editor.IStandaloneCodeEditor,
  host: CommentHost,
): MonacoReviewApi {
  ensureStyles();
  const MouseTargetType = monaco.editor.MouseTargetType;

  let comments: ReviewCommentLite[] = [];
  let drag: DragState | null = null;
  let composer: ComposerState | null = null;
  let hover = 0;
  let disposed = false;

  const decorations = editor.createDecorationsCollection();
  let zoneIds: string[] = [];
  /** Zones whose height must be measured after Monaco mounts their DOM. */
  let pendingMeasure: { id: string; node: HTMLElement; zone: { heightInPx: number } }[] = [];

  const docLines = (): number => editor.getModel()?.getLineCount() ?? 0;

  function renderDecorations(): void {
    const model = editor.getModel();
    if (!model) return;
    const lines = model.getLineCount();
    const decos: monacoApi.editor.IModelDeltaDecoration[] = [];
    const push = (n: number, options: monacoApi.editor.IModelDecorationOptions): void => {
      decos.push({ range: new monaco.Range(n, 1, n, 1), options });
    };
    for (const c of clampComments(comments, lines)) {
      for (let n = c.fromLine; n <= c.toLine; n++) {
        push(n, {
          isWholeLine: true,
          className: "rc-covered",
          glyphMarginClassName: n === c.fromLine ? "rc-glyph-anchor" : "rc-glyph-dot",
        });
      }
    }
    if (drag) {
      const a = Math.max(1, Math.min(drag.anchor, drag.current));
      const b = Math.min(lines, Math.max(drag.anchor, drag.current));
      for (let n = a; n <= b; n++) push(n, { isWholeLine: true, className: "rc-selected" });
    }
    if (composer && composer.from >= 1 && composer.from <= lines) {
      const to = Math.min(composer.to, lines);
      for (let n = composer.from; n <= to; n++) push(n, { isWholeLine: true, className: "rc-selected" });
    }
    // hover + only when the line is not covered and nothing else owns the gutter
    if (hover >= 1 && hover <= lines && !drag && !composer) {
      const covered = clampComments(comments, lines).some((c) => hover >= c.fromLine && hover <= c.toLine);
      if (!covered) push(hover, { isWholeLine: false, glyphMarginClassName: "rc-glyph-plus" });
    }
    decorations.set(decos);
  }

  function renderZones(): void {
    const lines = docLines();
    pendingMeasure = [];
    editor.changeViewZones((accessor) => {
      for (const id of zoneIds) accessor.removeZone(id);
      zoneIds = [];
      const addZone = (afterLine: number, node: HTMLElement): void => {
        const wrap = document.createElement("div");
        wrap.className = "rc-zone";
        wrap.appendChild(node);
        const zone = { afterLineNumber: afterLine, heightInPx: 96, domNode: wrap };
        const id = accessor.addZone(zone);
        zoneIds.push(id);
        pendingMeasure.push({ id, node: wrap, zone });
      };
      for (const c of clampComments(comments, lines)) {
        addZone(c.toLine, buildCard(c, (cid) => host.remove(cid)));
      }
      if (composer && composer.from >= 1 && composer.from <= lines) {
        const to = Math.min(composer.to, lines);
        addZone(
          to,
          buildComposer(composer.from, to, {
            save: (note) => {
              host.save(composer!.from, Math.min(composer!.to, docLines()), note);
              setComposer(null);
            },
            cancel: () => setComposer(null),
          }),
        );
      }
    });
    // zones need explicit pixel heights — measure the real DOM, then re-layout
    requestAnimationFrame(() => {
      if (disposed || !pendingMeasure.length) return;
      const toFix = pendingMeasure.filter((p) => {
        const h = p.node.offsetHeight;
        if (h > 0 && Math.abs(h - p.zone.heightInPx) > 1) {
          p.zone.heightInPx = h;
          return true;
        }
        return false;
      });
      if (toFix.length) {
        editor.changeViewZones((accessor) => {
          for (const p of toFix) accessor.layoutZone(p.id);
        });
      }
    });
  }

  function renderAll(): void {
    renderDecorations();
    renderZones();
  }

  function setComposer(next: ComposerState | null): void {
    composer = next;
    renderAll();
  }

  function startDrag(anchor: number): void {
    drag = { anchor, current: anchor };
    renderDecorations();
    const move = (e: MouseEvent): void => {
      const t = editor.getTargetAtClientPoint(e.clientX, e.clientY);
      const line = t?.position?.lineNumber;
      if (line && drag && drag.current !== line) {
        drag = { ...drag, current: line };
        renderDecorations();
      }
    };
    const up = (): void => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const d = drag;
      drag = null;
      if (d) setComposer({ from: Math.min(d.anchor, d.current), to: Math.max(d.anchor, d.current) });
      else renderAll();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  const subs: monacoApi.IDisposable[] = [
    editor.onMouseMove((e) => {
      if (drag) return; // drag owns the pointer
      const n = e.target.position?.lineNumber ?? 0;
      if (n !== hover) {
        hover = n;
        renderDecorations();
      }
    }),
    editor.onMouseLeave(() => {
      if (hover !== 0 && !drag) {
        hover = 0;
        renderDecorations();
      }
    }),
    editor.onMouseDown((e) => {
      if (e.target.type !== MouseTargetType.GUTTER_GLYPH_MARGIN) return;
      const n = e.target.position?.lineNumber;
      if (!n || composer) return;
      const covered = clampComments(comments, docLines()).some((c) => n >= c.fromLine && n <= c.toLine);
      if (covered) return;
      e.event.preventDefault();
      e.event.stopPropagation();
      startDrag(n);
    }),
    // edits move lines under comments — re-clamp and re-anchor zones
    editor.onDidChangeModelContent(() => renderAll()),
  ];

  return {
    setComments(next) {
      comments = next;
      renderAll();
    },
    openComposer(fromLine, toLine) {
      setComposer({ from: Math.min(fromLine, toLine), to: Math.max(fromLine, toLine) });
    },
    dispose() {
      disposed = true;
      for (const s of subs) s.dispose();
      decorations.clear();
      editor.changeViewZones((accessor) => {
        for (const id of zoneIds) accessor.removeZone(id);
      });
      zoneIds = [];
    },
  };
}
