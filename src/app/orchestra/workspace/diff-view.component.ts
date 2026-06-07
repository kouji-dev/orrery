import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { Agent, AgentFile, FileDiff } from "../models";
import { IconComponent } from "../shared/icon.component";
import { AgentsStore } from "../stores/agents.store";
import { fileDir, fileName, fileStateLabel, hunkHeader, langTag, mix } from "../utils";
import { CodeDiffComponent } from "./code-diff.component";

const LIST_MIN = 160; // px — narrowest the file-list panel may get
const LIST_MAX = 520; // px — widest before the diff body is too cramped
const LIST_DEFAULT = 236;

@Component({
  selector: "app-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CodeDiffComponent],
  template: `
    <div
      class="diff-grid"
      [class.resizing]="dragging()"
      [style.grid-template-columns]="listW() + 'px 6px 1fr'"
    >
      <!-- file list -->
      <div class="scroll-y" style="background:var(--panel);padding:6px 0">
        <div style="display:flex;align-items:center;gap:8px;padding:6px 10px 6px 14px">
          <span class="up" style="font-size:9px;color:var(--ink-3)">Changed · {{ changes().length }}</span>
          <!-- tree / flat toggle -->
          <div style="margin-left:auto;display:flex;gap:2px;padding:2px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
            <button
              class="btn"
              (click)="treeMode.set(true)"
              title="Tree view"
              [style.background]="treeMode() ? 'var(--panel-3)' : 'transparent'"
              [style.color]="treeMode() ? 'var(--ink)' : 'var(--ink-3)'"
              [style.box-shadow]="treeMode() ? '0 0 0 1px var(--hair-2)' : 'none'"
              style="padding:3px 7px;border-radius:4px;gap:5px;font-size:10px"
            ><app-icon name="graph" size="sm" [px]="12" [color]="treeMode() ? 'var(--accent)' : null" />Tree</button>
            <button
              class="btn"
              (click)="treeMode.set(false)"
              title="Flattened view"
              [style.background]="!treeMode() ? 'var(--panel-3)' : 'transparent'"
              [style.color]="!treeMode() ? 'var(--ink)' : 'var(--ink-3)'"
              [style.box-shadow]="!treeMode() ? '0 0 0 1px var(--hair-2)' : 'none'"
              style="padding:3px 7px;border-radius:4px;gap:5px;font-size:10px"
            ><app-icon name="dots" size="sm" [px]="12" [color]="!treeMode() ? 'var(--accent)' : null" />Flat</button>
          </div>
        </div>

        @if (!changes().length) {
          <div style="padding:10px 14px;font-size:10.5px;color:var(--ink-4)">no changes</div>
        } @else if (treeMode()) {
          <!-- tree view: folders + indented file leaves -->
          @for (row of treeRows(); track row.path) {
            @if (row.dir) {
              <div (click)="toggleDir(row.path)" style="display:flex;align-items:center;gap:6px;padding:4px 8px;cursor:pointer" [style.padding-left.px]="8 + row.depth * 13">
                <app-icon [name]="isDirOpen(row.path) ? 'chevronD' : 'chevron'" size="sm" [px]="11" color="var(--ink-4)" />
                <app-icon [name]="isDirOpen(row.path) ? 'folderOpen' : 'folder'" size="sm" [px]="13" color="var(--ink-4)" />
                <span style="font-size:11px;color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ row.name }}</span>
              </div>
            } @else {
              <div
                class="diff-file"
                [class.sel]="current()?.path === row.path"
                (click)="selPath.set(row.path)"
                [style.padding-left.px]="12 + row.depth * 13"
                style="display:flex;align-items:center;gap:7px;padding:4px 10px;cursor:pointer;margin:1px 6px;border-radius:var(--r-sm)"
              >
                <span [style.color]="stateInk(row.file!.state)" [style.background]="stateBg(row.file!.state)" style="flex:none;width:14px;height:14px;border-radius:3px;display:grid;place-items:center;font-size:9px;font-weight:700">{{ row.file!.state }}</span>
                <span [title]="row.file!.state === 'R' && row.file!.oldPath ? ('renamed from ' + row.file!.oldPath) : row.name" style="flex:1;min-width:0;font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ row.name }}</span>
                <span class="tnum" style="font-size:9.5px;display:flex;gap:4px;flex:none">
                  <span style="color:var(--code-add-ink)">+{{ row.file!.add }}</span>
                  @if (row.file!.del > 0) { <span style="color:var(--code-del-ink)">−{{ row.file!.del }}</span> }
                </span>
              </div>
            }
          }
        } @else {
          <!-- flat view -->
          @for (f of changes(); track f.path) {
            <div
              class="diff-file"
              [class.sel]="current()?.path === f.path"
              (click)="selPath.set(f.path)"
              style="display:flex;align-items:center;gap:8px;padding:6px 12px;cursor:pointer;margin:1px 6px;border-radius:var(--r-sm)"
            >
              <span
                [style.color]="stateInk(f.state)"
                [style.background]="stateBg(f.state)"
                style="flex:none;width:14px;height:14px;border-radius:3px;display:grid;place-items:center;font-size:9px;font-weight:700"
              >{{ f.state }}</span>
              <div style="flex:1;min-width:0">
                <div style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fname(f.path) }}</div>
                @if (f.state === 'R' && f.oldPath) {
                  <div [title]="'renamed from ' + f.oldPath" style="font-size:9.5px;color:var(--accent);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">← {{ f.oldPath }}</div>
                } @else if (fdir(f.path)) {
                  <div style="font-size:9.5px;color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fdir(f.path) }}</div>
                }
              </div>
              <span class="tnum" style="font-size:9.5px;display:flex;gap:4px;flex:none">
                <span style="color:var(--code-add-ink)">+{{ f.add }}</span>
                @if (f.del > 0) { <span style="color:var(--code-del-ink)">−{{ f.del }}</span> }
              </span>
            </div>
          }
        }
      </div>

      <!-- resizable separator: drag to rebalance the file list vs the diff body -->
      <div
        class="diff-resizer"
        role="separator"
        aria-orientation="vertical"
        (pointerdown)="startDrag($event)"
        (dblclick)="resetWidth()"
        title="Drag to resize · double-click to reset"
      ><span class="grip"></span></div>

      <!-- diff body -->
      <div style="display:flex;flex-direction:column;min-height:0;min-width:0;background:var(--bg)">
        <!-- diff header: LEFT = hunk header + status · RIGHT = language tag -->
        <div class="diff-head">
          <div class="diff-head-l">
            @if (current(); as f) {
              <div class="diff-head-top tnum">
                <span class="hunk">{{ headerHunk() }}</span>
                <span class="state-label" [style.color]="stateInk(f.state)">{{ stateLabel(f.state) }}</span>
                <span class="counts tnum">
                  <span style="color:var(--code-add-ink)">+{{ addCount() }}</span>
                  @if (delCount() > 0) { <span style="color:var(--code-del-ink)">−{{ delCount() }}</span> }
                </span>
              </div>
              <div class="diff-head-path">
                <app-icon name="file" size="sm" color="var(--ink-3)" />
                <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="f.path">{{ f.path }}</span>
                @if (loading()) { <span class="chip tnum" style="font-size:9px;padding:0 6px">loading…</span> }
              </div>
            } @else {
              <span style="color:var(--ink-4);font-size:11.5px">—</span>
            }
          </div>
          @if (current() && langLabel()) {
            <span class="lang-tag tnum">{{ langLabel() }}</span>
          }
        </div>

        @if (current() && diff(); as d) {
          <app-code-diff style="flex:1;min-height:0" [oldText]="d.old" [newText]="d.new" [lang]="d.lang" />
        } @else if (!current()) {
          <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:12px">no changed files</div>
        } @else if (!loading()) {
          <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:12px">no diff</div>
        }
      </div>
    </div>
  `,
  styles: [
    `
      .diff-grid {
        flex: 1;
        display: grid;
        min-height: 0;
        height: 100%;
      }
      /* while dragging, kill the iframe/text selection + pointer noise */
      .diff-grid.resizing {
        user-select: none;
        cursor: col-resize;
      }
      .diff-file:hover:not(.sel) {
        background: var(--panel-2);
      }
      .diff-file.sel {
        background: var(--panel-3);
      }

      /* ----- resizer handle ----- */
      .diff-resizer {
        position: relative;
        cursor: col-resize;
        display: grid;
        place-items: center;
        background: var(--panel);
        border-right: 1px solid var(--hair);
        touch-action: none;
      }
      .diff-resizer .grip {
        width: 1px;
        height: 28px;
        border-radius: 1px;
        background: var(--hair-2);
        transition: background 0.13s ease, box-shadow 0.13s ease;
      }
      .diff-resizer:hover .grip,
      .diff-grid.resizing .diff-resizer .grip {
        background: var(--accent);
        box-shadow: 0 0 8px -1px rgba(var(--accent-rgb), 0.7);
      }

      /* ----- diff header ----- */
      .diff-head {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 7px 14px;
        background: var(--panel);
        border-bottom: 1px solid var(--hair);
      }
      .diff-head-l {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 3px;
      }
      .diff-head-top {
        display: flex;
        align-items: center;
        gap: 9px;
        font-size: 11px;
        min-width: 0;
      }
      .hunk {
        color: var(--accent-2);
        background: color-mix(in oklch, var(--accent-2), transparent 88%);
        border: 1px solid color-mix(in oklch, var(--accent-2), transparent 70%);
        border-radius: var(--r-sm);
        padding: 1px 7px;
        white-space: nowrap;
      }
      .state-label {
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.1em;
        font-weight: 600;
      }
      .counts {
        display: flex;
        gap: 6px;
        font-size: 10.5px;
        margin-left: auto;
      }
      .diff-head-path {
        display: flex;
        align-items: center;
        gap: 7px;
        font-size: 11.5px;
        color: var(--ink-2);
        min-width: 0;
      }
      .lang-tag {
        flex: none;
        align-self: flex-start;
        font-size: 9.5px;
        text-transform: uppercase;
        letter-spacing: 0.1em;
        color: #06070b;
        background: linear-gradient(120deg, var(--accent), var(--accent-2));
        border-radius: 999px;
        padding: 3px 10px;
        font-weight: 600;
        white-space: nowrap;
      }
      [data-theme="light"] .lang-tag {
        color: #fff;
      }
    `,
  ],
})
export class DiffViewComponent {
  private agents = inject(AgentsStore);
  readonly agent = input.required<Agent>();
  // selection by PATH (works across both flat + tree views); treeMode toggles them
  readonly selPath = signal<string | null>(null);
  readonly treeMode = signal(false);

  readonly fname = fileName;
  readonly fdir = fileDir;
  readonly stateLabel = fileStateLabel;

  readonly changes = computed(() => this.agent().git_changes?.files ?? []);
  // the effectively-selected file: the one matching selPath, else the first
  readonly current = computed<AgentFile | undefined>(() => {
    const cs = this.changes();
    return cs.find((f) => f.path === this.selPath()) ?? cs[0];
  });
  // nested folder tree of the changed files, for the "Tree" view
  readonly diffTree = computed<DiffNode[]>(() => buildDiffTree(this.changes()));
  // collapsed folders (path → false); default: folders are OPEN
  readonly dirOpen = signal<Record<string, boolean>>({});
  // the visible rows: walk the tree, skipping children of collapsed folders
  readonly treeRows = computed<DiffRow[]>(() => {
    const open = this.dirOpen();
    const out: DiffRow[] = [];
    const walk = (nodes: DiffNode[], depth: number) => {
      for (const n of nodes) {
        out.push({ dir: n.dir, name: n.name, path: n.path, depth, file: n.file });
        if (n.dir && open[n.path] !== false) walk(n.children, depth + 1);
      }
    };
    walk(this.diffTree(), 0);
    return out;
  });
  isDirOpen(path: string): boolean {
    return this.dirOpen()[path] !== false;
  }
  toggleDir(path: string) {
    this.dirOpen.update((m) => ({ ...m, [path]: !(m[path] !== false) }));
  }
  readonly diff = signal<FileDiff | null>(null);
  readonly loading = signal(false);
  private gen = 0;
  private lastId: string | null = null;

  // ----- header derivations -----
  readonly langLabel = computed(() => {
    const f = this.current();
    return f ? langTag(f.path) : "";
  });
  // hunk header from the loaded diff content (falls back to file state pre-load)
  readonly headerHunk = computed(() => {
    const d = this.diff();
    if (d) return hunkHeader(d.old, d.new);
    const f = this.current();
    if (f?.state === "A") return "@@ -0,0 +1,? @@";
    if (f?.state === "D") return "@@ -1,? +0,0 @@";
    return "@@ … @@";
  });
  // counts: prefer the exact diff content, fall back to the backend file stat
  readonly addCount = computed(() => {
    const d = this.diff();
    if (d) return countLines(d.new) - sharedLines(d.old, d.new);
    return this.current()?.add ?? 0;
  });
  readonly delCount = computed(() => {
    const d = this.diff();
    if (d) return countLines(d.old) - sharedLines(d.old, d.new);
    return this.current()?.del ?? 0;
  });

  // ----- resizable separator (signal-backed width, pointer drag) -----
  readonly listW = signal(LIST_DEFAULT);
  readonly dragging = signal(false);
  private dragStartX = 0;
  private dragStartW = 0;

  constructor() {
    // reset selection to the first file when switching agents
    effect(() => {
      const id = this.agent().id;
      if (id !== this.lastId) {
        this.lastId = id;
        this.selPath.set(null); // current() falls back to the first changed file
      }
    });
    // load the diff for the selected file (superseded on rapid changes)
    effect(() => {
      const ag = this.agent();
      const f = this.current();
      if (!f) {
        this.diff.set(null);
        return;
      }
      const g = ++this.gen;
      this.loading.set(true);
      void this.agents
        .diff(ag.id, f.path, f.oldPath)
        .then((d) => {
          if (this.gen === g) {
            this.diff.set(d);
            this.loading.set(false);
          }
        })
        .catch(() => {
          if (this.gen === g) {
            this.diff.set(null);
            this.loading.set(false);
          }
        });
    });
  }

  startDrag(ev: PointerEvent) {
    ev.preventDefault();
    this.dragging.set(true);
    this.dragStartX = ev.clientX;
    this.dragStartW = this.listW();
    const target = ev.target as HTMLElement;
    target.setPointerCapture?.(ev.pointerId);
    const move = (e: PointerEvent) => {
      const next = this.dragStartW + (e.clientX - this.dragStartX);
      this.listW.set(Math.min(LIST_MAX, Math.max(LIST_MIN, next)));
    };
    const up = (e: PointerEvent) => {
      this.dragging.set(false);
      target.releasePointerCapture?.(e.pointerId);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
  resetWidth() {
    this.listW.set(LIST_DEFAULT);
  }

  stateInk(state: string): string {
    return state === "A"
      ? "var(--code-add-ink)"
      : state === "D"
        ? "var(--code-del-ink)"
        : state === "R"
          ? "var(--accent)"
          : "var(--accent-2)";
  }
  stateBg(state: string): string {
    return state === "A"
      ? "var(--code-add-bg)"
      : state === "D"
        ? "var(--code-del-bg)"
        : state === "R"
          ? mix("var(--accent)", 86)
          : mix("var(--accent-2)", 86);
  }
}

// ----- "Tree" view: build a nested folder tree from the changed-file paths -----
interface DiffRow {
  dir: boolean;
  name: string;
  path: string;
  depth: number;
  file?: AgentFile;
}
interface DiffNode {
  dir: boolean;
  name: string;
  path: string;
  file?: AgentFile;
  children: DiffNode[];
}

// Dirs first then files, alphabetical at each level. File leaves carry their
// AgentFile for state/counts. The component flattens this respecting per-folder
// open state (collapsible).
function buildDiffTree(files: AgentFile[]): DiffNode[] {
  const root: DiffNode = { dir: true, name: "", path: "", children: [] };
  const dirAt = new Map<string, DiffNode>([["", root]]);
  for (const f of files) {
    const parts = f.path.split("/");
    let parentPath = "";
    parts.forEach((part, i) => {
      const isFile = i === parts.length - 1;
      const path = parentPath ? `${parentPath}/${part}` : part;
      if (isFile) {
        dirAt.get(parentPath)!.children.push({ dir: false, name: part, path, file: f, children: [] });
      } else if (!dirAt.has(path)) {
        const node: DiffNode = { dir: true, name: part, path, children: [] };
        dirAt.get(parentPath)!.children.push(node);
        dirAt.set(path, node);
      }
      parentPath = path;
    });
  }
  const sortRec = (nodes: DiffNode[]) => {
    nodes.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
    for (const n of nodes) if (n.dir) sortRec(n.children);
  };
  sortRec(root.children);
  return root.children;
}

// ----- line-count helpers (header counts) -----
function countLines(s: string): number {
  return s.length ? s.replace(/\n$/, "").split("\n").length : 0;
}
// crude shared-line estimate: lines present (as a multiset) in both old and new
function sharedLines(oldText: string, newText: string): number {
  if (!oldText.length || !newText.length) return 0;
  const counts = new Map<string, number>();
  for (const l of oldText.replace(/\n$/, "").split("\n")) {
    counts.set(l, (counts.get(l) ?? 0) + 1);
  }
  let shared = 0;
  for (const l of newText.replace(/\n$/, "").split("\n")) {
    const c = counts.get(l) ?? 0;
    if (c > 0) {
      shared++;
      counts.set(l, c - 1);
    }
  }
  return shared;
}
