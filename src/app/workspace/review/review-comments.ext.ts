import type { EditorState, Extension, StateEffect, StateEffectType, Range } from "@codemirror/state";
import type { Decoration as DecorationT, DecorationSet, EditorView as EditorViewT } from "@codemirror/view";
import type { CMCore } from "../code-lang";

/**
 * Inline-review comments as a CodeMirror extension: hover-plus gutter, drag
 * range select, inline composer and saved-comment cards as block widgets.
 *
 * Framework-free by design — the host (an Angular component) pushes comments in
 * via {@link ReviewCommentsApi.setComments} and receives save/remove callbacks.
 *
 * IMPORTANT: CodeMirror is loaded at runtime from esm.sh (see code-lang.ts) —
 * a single shared instance is required for `instanceof` checks. So this module
 * must NOT import @codemirror/* values statically; it receives the loaded
 * modules through {@link CMCore} and only uses type-only imports.
 */

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

export interface ReviewCommentsApi {
  extension: Extension;
  /** Push the current comment list into a live view (idempotent). */
  setComments(view: EditorViewT, comments: ReviewCommentLite[]): void;
  /** Open the inline composer on a line range (also the drag-release target). */
  openComposer(view: EditorViewT, fromLine: number, toLine: number): void;
}

/** Drop comments that no longer fit the document; clamp partial overhangs. */
export function clampComments(comments: ReviewCommentLite[], docLines: number): ReviewCommentLite[] {
  return comments
    .filter((c) => c.fromLine >= 1 && c.fromLine <= docLines)
    .map((c) => (c.toLine <= docLines ? c : { ...c, toLine: docLines }));
}

// ----- tiny inline icons (lucide paths — matches IconComponent's set) -----
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
const PLUS = ["M5 12h14", "M12 5v14"];
const CHAT = ["M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"];
const TRASH = ["M3 6h18", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2", "M10 11v6", "M14 11v6"];

interface DragState {
  anchor: number; // line number where the drag started
  current: number; // line number under the pointer
}
interface ComposerState {
  from: number;
  to: number;
}

export function reviewCommentsExt(cm: CMCore, host: CommentHost): ReviewCommentsApi {
  const { StateEffect, StateField } = cm.state;
  const { EditorView, Decoration, WidgetType, gutter, GutterMarker } = cm.view;

  const setCommentsFx: StateEffectType<ReviewCommentLite[]> = StateEffect.define();
  const setDragFx: StateEffectType<DragState | null> = StateEffect.define();
  const setComposerFx: StateEffectType<ComposerState | null> = StateEffect.define();
  const setHoverFx: StateEffectType<number> = StateEffect.define(); // hovered line, 0 = none

  const relevant = (tr: { effects: readonly StateEffect<unknown>[] }) =>
    tr.effects.some((e) => e.is(setCommentsFx) || e.is(setDragFx) || e.is(setComposerFx));

  const commentsField = StateField.define<ReviewCommentLite[]>({
    create: () => [],
    update(v, tr) {
      for (const e of tr.effects) if (e.is(setCommentsFx)) v = e.value;
      return v;
    },
  });
  const dragField = StateField.define<DragState | null>({
    create: () => null,
    update(v, tr) {
      for (const e of tr.effects) if (e.is(setDragFx)) v = e.value;
      return v;
    },
  });
  const composerField = StateField.define<ComposerState | null>({
    create: () => null,
    update(v, tr) {
      for (const e of tr.effects) if (e.is(setComposerFx)) v = e.value;
      return v;
    },
  });
  const hoverField = StateField.define<number>({
    create: () => 0,
    update(v, tr) {
      for (const e of tr.effects) if (e.is(setHoverFx)) v = e.value;
      return v;
    },
  });

  // ----- block widgets -----

  class CardWidget extends WidgetType {
    constructor(readonly c: ReviewCommentLite) {
      super();
    }
    override eq(o: CardWidget): boolean {
      return o.c.id === this.c.id && o.c.note === this.c.note && o.c.fromLine === this.c.fromLine && o.c.toLine === this.c.toLine;
    }
    override toDOM(view: EditorViewT): HTMLElement {
      const c = this.c;
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
      del.addEventListener("click", () => host.remove(c.id));
      head.append(avatar, who, ref, chip, del);
      body.appendChild(head);

      const note = document.createElement("div");
      note.className = "rc-card-note";
      note.textContent = c.note;
      body.appendChild(note);
      void view;
      return root;
    }
    override ignoreEvent(): boolean {
      return true;
    }
  }

  class ComposerWidget extends WidgetType {
    constructor(readonly from: number, readonly to: number) {
      super();
    }
    override eq(o: ComposerWidget): boolean {
      return o.from === this.from && o.to === this.to;
    }
    override toDOM(view: EditorViewT): HTMLElement {
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
      const range = this.to > this.from ? `lines ${this.from}–${this.to}` : `line ${this.from}`;
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
      const cancel = document.createElement("button");
      cancel.className = "btn ghost-hair rc-composer-btn";
      cancel.textContent = "Cancel";
      const save = document.createElement("button");
      save.className = "btn primary rc-composer-btn";
      save.textContent = "Save";
      save.disabled = true;
      foot.append(hint, spacer, cancel, save);
      body.appendChild(foot);

      const doCancel = () => view.dispatch({ effects: setComposerFx.of(null) });
      const doSave = () => {
        const note = ta.value.trim();
        if (!note) return;
        host.save(this.from, this.to, note);
        view.dispatch({ effects: setComposerFx.of(null) });
      };
      ta.addEventListener("input", () => (save.disabled = !ta.value.trim()));
      ta.addEventListener("keydown", (e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === "Enter") doSave();
        if (e.key === "Escape") doCancel();
      });
      cancel.addEventListener("click", doCancel);
      save.addEventListener("click", doSave);
      setTimeout(() => ta.focus(), 0);
      return root;
    }
    override ignoreEvent(): boolean {
      return true;
    }
  }

  // ----- decorations: covered-line tint, drag/composer range tint, widgets -----

  const coveredLine = Decoration.line({ class: "rc-covered" });
  const selectedLine = Decoration.line({ class: "rc-selected" });

  function buildDeco(state: EditorState): DecorationSet {
    const doc = state.doc;
    const ranges: Range<DecorationT>[] = [];
    for (const c of clampComments(state.field(commentsField), doc.lines)) {
      for (let n = c.fromLine; n <= c.toLine; n++) ranges.push(coveredLine.range(doc.line(n).from));
      ranges.push(Decoration.widget({ widget: new CardWidget(c), block: true, side: 1 }).range(doc.line(c.toLine).to));
    }
    const d = state.field(dragField);
    if (d) {
      const a = Math.max(1, Math.min(d.anchor, d.current));
      const b = Math.min(doc.lines, Math.max(d.anchor, d.current));
      for (let n = a; n <= b; n++) ranges.push(selectedLine.range(doc.line(n).from));
    }
    const comp = state.field(composerField);
    if (comp && comp.from >= 1 && comp.from <= doc.lines) {
      const to = Math.min(comp.to, doc.lines);
      for (let n = comp.from; n <= to; n++) ranges.push(selectedLine.range(doc.line(n).from));
      ranges.push(Decoration.widget({ widget: new ComposerWidget(comp.from, to), block: true, side: 2 }).range(doc.line(to).to));
    }
    return Decoration.set(ranges, true);
  }

  const decoField = StateField.define<DecorationSet>({
    create: (state) => buildDeco(state),
    update(deco, tr) {
      if (tr.docChanged || relevant(tr)) return buildDeco(tr.state);
      return deco;
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  // ----- gutter: hover +, anchor chip, range dot -----

  class PlusMarker extends GutterMarker {
    constructor(readonly line: number) {
      super();
    }
    override eq(o: PlusMarker): boolean {
      return o.line === this.line;
    }
    override toDOM(view: EditorViewT): Node {
      const btn = document.createElement("button");
      btn.className = "rc-plus";
      btn.title = "Comment on this line — or drag down to select a range";
      btn.appendChild(svgIcon(PLUS, 11));
      btn.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        startDrag(view, this.line);
      });
      return btn;
    }
  }
  class AnchorMarker extends GutterMarker {
    override eq(): boolean {
      return true;
    }
    override toDOM(): Node {
      const chip = document.createElement("span");
      chip.className = "rc-anchor";
      chip.title = "comment saved";
      chip.appendChild(svgIcon(CHAT, 10));
      return chip;
    }
  }
  class DotMarker extends GutterMarker {
    override eq(): boolean {
      return true;
    }
    override toDOM(): Node {
      const dot = document.createElement("span");
      dot.className = "rc-dot";
      return dot;
    }
  }

  function startDrag(view: EditorViewT, anchor: number): void {
    view.dispatch({ effects: setDragFx.of({ anchor, current: anchor }) });
    const move = (e: MouseEvent) => {
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY }, false);
      const line = view.state.doc.lineAt(pos).number;
      const d = view.state.field(dragField);
      if (d && d.current !== line) view.dispatch({ effects: setDragFx.of({ ...d, current: line }) });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const d = view.state.field(dragField);
      const effects: StateEffect<unknown>[] = [setDragFx.of(null)];
      if (d) {
        effects.push(setComposerFx.of({ from: Math.min(d.anchor, d.current), to: Math.max(d.anchor, d.current) }));
      }
      view.dispatch({ effects });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  const commentGutter = gutter({
    class: "rc-gutter",
    lineMarker(view, block) {
      const line = view.state.doc.lineAt(block.from);
      if (block.from !== line.from) return null; // wrapped-line continuation rows
      const n = line.number;
      const comments = clampComments(view.state.field(commentsField), view.state.doc.lines);
      const cov = comments.find((c) => n >= c.fromLine && n <= c.toLine);
      if (cov) return n === cov.fromLine ? new AnchorMarker() : new DotMarker();
      if (view.state.field(composerField)) return null;
      const d = view.state.field(dragField);
      if (d) return null; // range tint carries the feedback while dragging
      return view.state.field(hoverField) === n ? new PlusMarker(n) : null;
    },
    lineMarkerChange: (update) => update.transactions.some((tr) => relevant(tr) || tr.effects.some((e) => e.is(setHoverFx))),
    initialSpacer: () => new PlusMarker(1),
  });

  // Row hover → show the + in the gutter for that line (GitHub-style). A plain
  // CM transaction per crossed line: no doc change, gutter-only update — cheap.
  const hoverHandlers = EditorView.domEventHandlers({
    mousemove(e, view) {
      if (view.state.field(dragField)) return; // drag owns the pointer
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
      const n = pos == null ? 0 : view.state.doc.lineAt(pos).number;
      if (view.state.field(hoverField) !== n) view.dispatch({ effects: setHoverFx.of(n) });
    },
    mouseleave(_e, view) {
      if (view.state.field(hoverField) !== 0 && !view.state.field(dragField)) {
        view.dispatch({ effects: setHoverFx.of(0) });
      }
    },
  });

  const theme = EditorView.theme({
    ".rc-gutter": { width: "24px" },
    ".rc-gutter .cm-gutterElement": { display: "flex", alignItems: "center", justifyContent: "center" },
    ".rc-plus": {
      width: "16px",
      height: "16px",
      borderRadius: "4px",
      border: "none",
      display: "grid",
      placeItems: "center",
      cursor: "grab",
      background: "var(--accent)",
      color: "#06070b",
      padding: "0",
    },
    ".rc-anchor": {
      width: "15px",
      height: "15px",
      borderRadius: "4px",
      display: "grid",
      placeItems: "center",
      background: "var(--accent)",
      color: "#06070b",
    },
    ".rc-dot": {
      width: "5px",
      height: "5px",
      borderRadius: "50%",
      background: "color-mix(in oklch, var(--accent), transparent 30%)",
    },
    ".rc-covered": { boxShadow: "inset 2px 0 0 var(--accent)" },
    ".rc-selected": { background: "color-mix(in oklch, var(--accent), transparent 84%)" },
    ".rc-card": {
      display: "flex",
      background: "var(--panel)",
      borderTop: "1px solid var(--hair)",
      borderBottom: "1px solid var(--hair)",
    },
    ".rc-card-bar": {
      width: "24px",
      flex: "none",
      background: "color-mix(in oklch, var(--accent), transparent 86%)",
      boxShadow: "inset 2px 0 0 var(--accent)",
    },
    ".rc-card-body": { flex: "1", padding: "9px 14px 9px 12px", minWidth: "0" },
    ".rc-card-head": { display: "flex", alignItems: "center", gap: "7px" },
    ".rc-card-avatar": {
      width: "17px",
      height: "17px",
      flex: "none",
      borderRadius: "50%",
      display: "grid",
      placeItems: "center",
      fontSize: "8.5px",
      fontWeight: "700",
      color: "var(--accent)",
      background: "color-mix(in oklch, var(--accent), transparent 84%)",
      border: "1px solid color-mix(in oklch, var(--accent), transparent 60%)",
    },
    ".rc-card-who": { fontSize: "11px", color: "var(--ink-2)" },
    ".rc-card-ref": { fontSize: "9.5px", color: "var(--ink-4)" },
    ".rc-card-chip": {
      fontSize: "8.5px",
      padding: "0 5px",
      color: "var(--accent)",
      border: "1px solid color-mix(in oklch, var(--accent), transparent 60%)",
      borderRadius: "999px",
    },
    ".rc-card-del": {
      marginLeft: "auto",
      background: "transparent",
      border: "none",
      color: "var(--ink-4)",
      cursor: "pointer",
      display: "flex",
      padding: "3px",
      borderRadius: "3px",
    },
    ".rc-card-del:hover": { color: "var(--code-del-ink)" },
    ".rc-card-note": {
      fontSize: "12px",
      color: "var(--ink)",
      lineHeight: "1.5",
      marginTop: "5px",
      whiteSpace: "pre-wrap",
      wordBreak: "break-word",
    },
    ".rc-composer": {
      display: "flex",
      background: "color-mix(in oklch, var(--accent), transparent 95%)",
      borderTop: "1px solid color-mix(in oklch, var(--accent), transparent 70%)",
      borderBottom: "1px solid var(--hair)",
    },
    ".rc-composer-bar": { width: "24px", flex: "none", background: "color-mix(in oklch, var(--accent), transparent 70%)" },
    ".rc-composer-body": { flex: "1", padding: "10px 14px 10px 12px" },
    ".rc-composer-head": { display: "flex", alignItems: "center", gap: "7px", marginBottom: "7px" },
    ".rc-composer-icon": { color: "var(--accent)" },
    ".rc-composer-label": { fontSize: "10.5px", color: "var(--ink-2)" },
    ".rc-composer-label b": { color: "var(--ink)" },
    ".rc-composer-tag": { marginLeft: "auto", fontSize: "8.5px", color: "var(--ink-4)" },
    ".rc-composer-ta": {
      width: "100%",
      resize: "vertical",
      minHeight: "56px",
      background: "var(--panel-2)",
      border: "1px solid var(--hair)",
      borderRadius: "var(--r-sm)",
      padding: "8px 10px",
      color: "var(--ink)",
      fontFamily: "var(--font-mono)",
      fontSize: "12px",
      lineHeight: "1.55",
      outline: "none",
    },
    ".rc-composer-foot": { display: "flex", alignItems: "center", gap: "8px", marginTop: "8px" },
    ".rc-composer-hint": { fontSize: "9.5px", color: "var(--ink-4)" },
    ".rc-composer-btn": { padding: "4px 12px", fontSize: "11px" },
  });

  return {
    extension: [commentsField, dragField, composerField, hoverField, decoField, commentGutter, hoverHandlers, theme],
    setComments(view, comments) {
      view.dispatch({ effects: setCommentsFx.of(comments) });
    },
    openComposer(view, fromLine, toLine) {
      view.dispatch({ effects: setComposerFx.of({ from: Math.min(fromLine, toLine), to: Math.max(fromLine, toLine) }) });
    },
  };
}
