import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { Agent, AgentFile, BlameIntern, BlameLine, FileDiff, hydrateBlame } from "../models";
import { IconComponent } from "../shared/icon.component";
import { GitActionBarComponent } from "./git-action-bar.component";
import { AgentsStore } from "../stores/agents.store";
import { AgentWorkStore } from "../agents/agent-work.store";
import { BRIDGE, Commands } from "../data-source/bridge";
import { fileDir, fileName, fileStateLabel, isMarkdownPath, langId, langTag, mix } from "../utils";
import { UiStore } from "../ui/ui.store";
import { UnifiedCodeComponent } from "./review/unified-code.component";
import { AnnotateBlameComponent } from "./review/annotate-blame.component";
import { SendReviewButtonComponent } from "./review/send-review.component";
import { DiffStats } from "./review/chunk-stats";
import { KjBadgeComponent, KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";
import { StateBadgeComponent } from "../shared/git/state-badge.component";
import { AddDelComponent } from "../shared/git/add-del.component";

const LIST_MIN = 160; // px — narrowest the file-list panel may get
const LIST_MAX = 520; // px — widest before the diff body is too cramped
const LIST_DEFAULT = 300;

@Component({
  selector: "app-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, UnifiedCodeComponent, AnnotateBlameComponent, SendReviewButtonComponent, GitActionBarComponent, KjBadgeComponent, KjButtonComponent, KjTabsComponent, KjTabListComponent, KjTabComponent, StateBadgeComponent, AddDelComponent],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;min-width:0">
    <div
      class="diff-grid"
      [class.resizing]="dragging()"
      [style.grid-template-columns]="listW() + 'px 6px 1fr'"
    >
      <!-- file list: header pinned, only the listing below scrolls -->
      <div style="display:flex;flex-direction:column;min-height:0;background:var(--panel);padding-top:var(--sp-3)">
        <div style="flex:none;display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-5) var(--sp-3) var(--sp-6)">
          <span class="up" style="color:var(--ink-3)">Changed · {{ changes().length }}</span>
          <!-- tree / flat toggle -->
          <kj-tabs variant="pills" class="tabs-xs" style="margin-left:auto"
                   [value]="treeMode() ? 'tree' : 'flat'" (valueChange)="setTreeMode($event === 'tree')">
            <kj-tab-list aria-label="File list layout">
              <kj-tab value="tree" title="Tree view"><app-icon size="md" name="graph" [color]="treeMode() ? 'var(--ui-ink)' : null" />Tree</kj-tab>
              <kj-tab value="flat" title="Flattened view"><app-icon size="md" name="dots" [color]="!treeMode() ? 'var(--ui-ink)' : null" />Flat</kj-tab>
            </kj-tab-list>
          </kj-tabs>
          <kj-button kjSize="icon" kjVariant="ghost" (click)="refresh()" title="Rescan changes"><app-icon size="md" name="refresh" /></kj-button>
        </div>

        <div class="scroll-y" style="padding-bottom:var(--sp-3)">
        @if (!changes().length) {
          <div style="padding:var(--sp-5) var(--sp-6);color:var(--ink-4)">no changes</div>
        } @else if (treeMode()) {
          <!-- tree view: folders + indented file leaves -->
          @for (row of treeRows(); track row.path) {
            @if (row.dir) {
              <div class="diff-dir" (click)="toggleDir(row.path)" [style.padding-left.px]="8 + row.depth * 13">
                <app-icon [name]="isDirOpen(row.path) ? 'chevronD' : 'chevron'" size="sm" color="var(--ink-4)" />
                <app-icon [name]="isDirOpen(row.path) ? 'folderOpen' : 'folder'" size="sm" color="var(--ink-4)" />
                <!-- uniform-state folders read like their files: a fully-deleted
                     folder is dim + strikethrough (never red — locked rule), a
                     fully-new / moved one carries the A / R chip -->
                <span
                  [style.color]="row.state === 'D' ? 'var(--ink-4)' : 'var(--ink-3)'"
                  [style.text-decoration]="row.state === 'D' ? 'line-through' : 'none'"
                  [style.opacity]="row.state === 'D' ? 0.7 : 1"
                  style="font-size:var(--fs-sm);overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
                >{{ row.name }}</span>
                @if (row.state) {
                  <span [style.color]="stateInk(row.state)" [style.background]="stateBg(row.state)" class="state-chip">{{ row.state }}</span>
                }
                <span class="tnum counts-chip" style="opacity:0.75">
                  <span style="color:var(--code-add-ink)">+{{ row.add }}</span>
                  @if ((row.del ?? 0) > 0) { <span style="color:var(--code-del-ink)">−{{ row.del }}</span> }
                </span>
              </div>
            } @else {
              <div
                class="diff-file list-row"
                [class.sel]="current()?.path === row.path"
                (click)="select(row.path)"
                [style.padding-left.px]="12 + row.depth * 13"
              >
                <app-state-badge [state]="row.file!.state" />
                <span [title]="row.file!.state === 'R' && row.file!.oldPath ? ('renamed from ' + row.file!.oldPath) : row.name" class="fname trunc">{{ row.name }}</span>
                <app-add-del [add]="row.file!.add" [del]="row.file!.del" style="margin-left:auto" />
              </div>
            }
          }
        } @else {
          <!-- flat view: SAME single-line row as the tree — the directory (or
               rename origin) rides inline, muted, so both modes share --row-h -->
          @for (f of changes(); track f.path) {
            <div
              class="diff-file list-row"
              [class.sel]="current()?.path === f.path"
              (click)="select(f.path)"
            >
              <app-state-badge [state]="f.state" />
              <span [title]="f.state === 'R' && f.oldPath ? ('renamed from ' + f.oldPath) : f.path" class="fname trunc">{{ fname(f.path) }}</span>
              @if (f.state === 'R' && f.oldPath) {
                <span class="fdir trunc" style="color:var(--vcs-renamed)">← {{ f.oldPath }}</span>
              } @else if (fdir(f.path)) {
                <span class="fdir trunc">{{ fdir(f.path) }}</span>
              }
              <app-add-del [add]="f.add" [del]="f.del" style="margin-left:auto" />
            </div>
          }
        }
        </div>
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
        <div class="pane-head diff-head">
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
                <span class="trunc" [title]="f.path">{{ f.path }}</span>
                @if (loading()) { <kj-badge class="tnum">loading…</kj-badge> }
              </div>
            } @else {
              <span style="color:var(--ink-4);font-size:var(--fs-meta)">—</span>
            }
          </div>
          <!-- Preview: previewable files (md) open rendered in the workspace,
               exactly as if clicked in the right files panel -->
          @if (canPreview()) {
            <kj-button kjVariant="outline" (click)="openPreview()" title="Preview — open the rendered file in the workspace" style="--kj-button-fg: var(--ink-3)">
              <app-icon size="md" name="file" />
              Preview
            </kj-button>
          }
          @if (current(); as f) {
            <kj-button kjVariant="outline" [kjPressed]="annotate()" (click)="annotate.set(!annotate())" title="Annotate — show who last changed each line on both sides">
              <app-icon size="md" name="git" [color]="annotate() ? 'var(--ui-ink)' : null" />
              Annotate
            </kj-button>
            <app-send-review-button [agent]="agent().id" [agentName]="agent().name" />
          }
          <!-- git ACTIONS live in the action bar docked under the diff (v2) —
               one home per verb; this header only reads. -->
          @if (current() && langLabel()) {
            <span class="chip tnum" style="align-self:flex-start;font-size:var(--fs-2xs)">{{ langLabel() }}</span>
          }
        </div>

        @if (current() && diff(); as d) {
          @if (annotate()) {
            <app-annotate-blame [lines]="newBlame()" (openCommit)="onOpenCommit($event)" />
          } @else {
            <app-unified-code [agent]="agent().id" [file]="current()!.path" view="diff" [oldText]="d.old" [newText]="d.new" [lang]="langId()" (stats)="stats.set($event)" />
          }
        } @else if (!current()) {
          <div class="pane-empty">no changed files</div>
        } @else if (!loading()) {
          <div class="pane-empty">no diff</div>
        }
      </div>
    </div>

    <!-- v2: every Act verb docked directly under the diff it judges -->
    <app-git-action-bar [agent]="agent()" />
    </div>
  `,
  styles: [
    `
      .diff-grid {
        flex: 1;
        display: grid;
        min-height: 0;
      }
      /* while dragging, kill the iframe/text selection + pointer noise */
      .diff-grid.resizing {
        user-select: none;
        cursor: col-resize;
      }
      /* ONE row recipe for both view modes — same height (density-scaled via
         --row-h), so Tree and Flat read as the same list, compact to comfy. */
      .diff-file,
      .diff-dir {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        height: var(--row-h);
        cursor: pointer;
      }
      /* margin / radius / cursor / hover / selected all come from .list-row */
      .diff-file {
        padding: 0 var(--sp-5);
      }
      .diff-dir {
        padding: 0 var(--sp-4);
      }
      .diff-file .state-chip,
      .diff-dir .state-chip {
        flex: none;
        width: var(--sp-6);
        height: var(--sp-6);
        border-radius: 3px;
        display: grid;
        place-items: center;
        font-size: var(--fs-2xs);
        font-weight: 700;
      }
      .diff-file .fname {
        flex: 0 1 auto;
        }
      .diff-file .fdir {
        flex: 1 1 auto;
        font-size: var(--fs-meta);
        color: var(--ink-4);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .diff-file .counts-chip,
      .diff-dir .counts-chip {
        font-size: var(--fs-2xs);
        display: flex;
        gap: var(--sp-2);
        flex: none;
        margin-left: auto;
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
        height: var(--ctl-h);
        border-radius: 1px;
        background: var(--hair-2);
        transition: background 0.13s ease, box-shadow 0.13s ease;
      }
      .diff-resizer:hover .grip,
      .diff-grid.resizing .diff-resizer .grip {
        background: var(--ui-fill);
        box-shadow: 0 0 0 1px var(--ui-sel);
      }

      /* ----- diff header: .pane-head + the deltas this one needs (a two-line
         left block, so the row aligns to the TOP rather than centre) ----- */
      .diff-head {
        align-items: flex-start;
        gap: var(--sp-6);
        padding-block: var(--sp-3);
        background: var(--panel);
      }
      .diff-head-l {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: var(--sp-1);
      }
      .diff-head-top {
        display: flex;
        align-items: center;
        gap: var(--sp-4);
        min-width: 0;
      }
      .hunk {
        color: var(--ink-2);
        background: var(--side-a);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
        padding: 1px var(--sp-3);
        white-space: nowrap;
      }
      .state-label {
        text-transform: uppercase;
        letter-spacing: 0.1em;
        font-weight: var(--fw-medium);
      }
      .counts {
        display: flex;
        gap: var(--sp-3);
        margin-left: auto;
      }
      .diff-head-path {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        color: var(--ink-2);
        min-width: 0;
      }
    `,
  ],
})
export class DiffViewComponent {
  private agents = inject(AgentsStore);
  private work = inject(AgentWorkStore);
  private ui = inject(UiStore);
  readonly agent = input.required<Agent>();

  // selection by PATH (works across both flat + tree views); treeMode toggles
  // them. Selection lives in UiStore keyed by agent — the pane destroys this
  // component on tab switch, so a local signal would forget the opened file.
  readonly selPath = computed(() => this.ui.diffSelectionFor(this.agentId()));
  select(path: string): void {
    this.ui.setDiffSelection(this.agentId(), path);
  }
  // Tree/Flat preference — global like diffListWidth, persisted with the
  // workspace (a component-local signal would reset on every tab switch).
  readonly treeMode = computed(() => this.ui.diffTreeMode());
  setTreeMode(on: boolean): void {
    this.ui.diffTreeMode.set(on);
  }

  readonly fname = fileName;
  readonly fdir = fileDir;
  readonly stateLabel = fileStateLabel;

  // Why id, not the Agent object: runtime.agents() re-creates agent objects on
  // every overlay patch (working/needsInput transitions while running), so
  // anything keyed on agent() identity re-fires for free. The id string is
  // stable — computed memoization stops the churn here for everything downstream.
  private readonly agentId = computed(() => this.agent().id);

  readonly changes = computed(() => this.work.changesFor(this.agentId()).data);
  // the effectively-selected file: the one matching selPath, else the first
  readonly current = computed<AgentFile | undefined>(() => {
    const cs = this.changes();
    return cs.find((f) => f.path === this.selPath()) ?? cs[0];
  });
  // nested folder tree of the changed files, for the "Tree" view
  readonly diffTree = computed<DiffNode[]>(() => buildDiffTree(this.changes()));
  // collapsed folders (path → false); default: folders are OPEN. Lives in
  // UiStore keyed by agent (persisted with the workspace) — the pane destroys
  // this component on every tab switch, so local state would reset each time.
  readonly dirOpen = computed<Record<string, boolean>>(() =>
    this.ui.diffDirOpenFor(this.agentId()),
  );
  // the visible rows: walk the tree, skipping children of collapsed folders
  readonly treeRows = computed<DiffRow[]>(() => {
    const open = this.dirOpen();
    const out: DiffRow[] = [];
    const walk = (nodes: DiffNode[], depth: number) => {
      for (const n of nodes) {
        out.push({ dir: n.dir, name: n.name, path: n.path, depth, file: n.file, state: n.state, add: n.add, del: n.del });
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
    this.ui.toggleDiffDir(this.agentId(), path);
  }
  /** Manual fallback: re-fetch this agent's changed files — the diff effect then
   *  reloads the selected file's content. */
  refresh() {
    this.work.loadChanges(this.agent().id);
  }
  readonly diff = signal<FileDiff | null>(null);
  readonly loading = signal(false);
  private gen = 0;
  /** agent:path the last load ran for — a change means a real SWITCH (reset
   *  header chrome); an equal key is a silent background refresh. */
  private loadedKey: string | null = null;

  // ----- annotate (blame) -----
  private bridge = inject(BRIDGE);
  /** Annotate toggle — overlays a committer gutter on both sides of the diff. */
  readonly annotate = signal(false);
  readonly newBlame = signal<BlameLine[]>([]);
  private blameGen = 0;

  // ----- inline review signals -----
  /** Exact header stats emitted by the diff editor's own line changes. */
  readonly stats = signal<DiffStats | null>(null);

  // ----- header derivations -----
  readonly langLabel = computed(() => {
    const f = this.current();
    return f ? langTag(f.path) : "";
  });
  // the CodeMirror grammar tag for the selected file (drives syntax highlighting)
  readonly langId = computed(() => {
    const f = this.current();
    return f ? langId(f.path) : "";
  });
  // hunk header from the merge view's diff (falls back to file state pre-load)
  readonly headerHunk = computed(() => {
    const s = this.stats();
    if (s) return s.hunks > 1 ? `${s.hunk} · ${s.hunks} hunks` : s.hunk;
    const f = this.current();
    if (f?.state === "A") return "@@ -0,0 +1,? @@";
    if (f?.state === "D") return "@@ -1,? +0,0 @@";
    return "@@ … @@";
  });
  // counts: prefer the merge view's exact diff, fall back to the backend file stat
  readonly addCount = computed(() => this.stats()?.add ?? this.current()?.add ?? 0);
  readonly delCount = computed(() => this.stats()?.del ?? this.current()?.del ?? 0);

  // ----- resizable separator (store-backed width, pointer drag) -----
  // The width preference lives in UiStore (persisted with the workspace), so
  // it survives tab switches AND restarts. null = the default; always clamped.
  readonly listW = computed(() => {
    const w = this.ui.diffListWidth();
    return w == null ? LIST_DEFAULT : Math.min(LIST_MAX, Math.max(LIST_MIN, w));
  });
  readonly dragging = signal(false);
  private dragStartX = 0;
  private dragStartW = 0;

  constructor() {
    // Load the changed-file list when it was never requested for this agent —
    // real agents are warmed by AgentRuntimeService on activation, but the v2
    // project pseudo-agent has no watcher/activation path, so the diff view
    // itself triggers the first scan (a no-op for already-loaded entries).
    effect(() => {
      const id = this.agentId();
      if (this.work.changesFor(id).status === "idle") this.work.loadChanges(id);
    });

    // Load the diff for the selected file (superseded on rapid changes).
    // Triggers: agent switch (id), selection change, or a refreshed changes
    // entry (push/pull → new file objects = content may differ). Reading the
    // id (not agent()) keeps runtime overlay patches from re-fetching the
    // diff when nothing changed.
    //
    // SILENT same-file refresh: watcher scans re-create the file objects on
    // every worktree event, so this effect re-fires constantly while an agent
    // works. Resetting stats/loading each time flashed the whole header once
    // per scan — now that chrome churns only when the agent/file actually
    // SWITCHED; a background refresh just refetches and lets the value-equal
    // diff signals swallow no-op results.
    effect(() => {
      const id = this.agentId();
      const f = this.current();
      // Every landed scan bumps this — the file entries themselves keep their
      // references when a scan changes no ± counts, yet the CONTENT may still
      // differ (an edit inside an already-modified line), so the refetch must
      // key on the scan, not on row identity.
      this.work.scanSeqFor(id);
      const key = f ? id + ":" + f.path : null;
      const switched = key !== this.loadedKey;
      this.loadedKey = key;
      if (switched) this.stats.set(null); // stale counts must not survive a file switch
      if (!f) {
        this.diff.set(null);
        return;
      }
      const g = ++this.gen;
      if (switched) this.loading.set(true);
      void this.agents
        .diff(id, f.path, f.oldPath)
        .then((d) => {
          if (this.gen === g) {
            // identical content keeps the object — a no-op refresh renders nothing
            const cur = this.diff();
            if (!cur || cur.old !== d.old || cur.new !== d.new) this.diff.set(d);
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

    // Load new-side blame when annotate is on (or the file/agent changes).
    // `working_blame` returns old (HEAD) + new (working-tree, via blame_buffer)
    // so each side of the diff gets correct authorship; uncommitted lines show
    // "Uncommitted". Superseded by gen guard on rapid switches.
    effect(() => {
      const on = this.annotate();
      const id = this.agentId();
      const f = this.current();
      if (!on || !f) {
        this.newBlame.set([]);
        return;
      }
      const g = ++this.blameGen;
      void this.bridge
        .invoke<{ old: BlameIntern; new: BlameIntern }>(Commands.AgentWorkingBlame, { id, path: f.path })
        .then((r) => {
          if (this.blameGen !== g) return;
          this.newBlame.set(hydrateBlame(r.new));
        })
        .catch(() => {
          if (this.blameGen !== g) return;
          this.newBlame.set([]);
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
      this.ui.diffListWidth.set(Math.min(LIST_MAX, Math.max(LIST_MIN, next)));
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
    this.ui.diffListWidth.set(null); // back to the default
  }

  onOpenCommit(sha: string) {
    this.ui.setGitView(this.agent().id, { kind: "commit", sha });
  }

  // ----- Preview: hand the selected file to the workspace file tab -----
  // Deleted files have no working-tree content to render, so no button for 'D'.
  readonly canPreview = computed(() => {
    const f = this.current();
    return !!f && f.state !== "D" && isMarkdownPath(f.path);
  });
  openPreview() {
    const f = this.current();
    if (f) this.ui.openFileInWorkspace(this.agent().id, f.path);
  }

  stateBg(state: string): string {
    return state === "A"
      ? mix("var(--vcs-added)", 88)
      : state === "D"
        ? "transparent"
        : state === "R"
          ? mix("var(--vcs-renamed)", 88)
          : mix("var(--vcs-modified)", 88);
  }

  stateInk(state: string): string {
    return state === "A"
      ? "var(--vcs-added)"
      : state === "D"
        ? "var(--vcs-deleted)"
        : state === "R"
          ? "var(--vcs-renamed)"
          : "var(--vcs-modified)";
  }
}

// ----- "Tree" view: build a nested folder tree from the changed-file paths -----
interface DiffRow {
  dir: boolean;
  name: string;
  path: string;
  depth: number;
  file?: AgentFile;
  /** Dirs: the uniform descendant state (A/D/R/M) — undefined when mixed. */
  state?: string;
  /** Dirs: aggregate line counts over every descendant file. */
  add?: number;
  del?: number;
}
interface DiffNode {
  dir: boolean;
  name: string;
  path: string;
  file?: AgentFile;
  children: DiffNode[];
  state?: string;
  add?: number;
  del?: number;
}

// Dirs first then files, alphabetical at each level. File leaves carry their
// AgentFile for state/counts; dir nodes carry the AGGREGATE — total ±lines and,
// when every descendant shares one state, that state (a fully-deleted folder
// reads as deleted, a fully-new one as added, a moved one as renamed). The
// component flattens this respecting per-folder open state (collapsible).
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
  aggregateRec(root.children);
  return root.children;
}

/** Bottom-up dir aggregation: ±line sums plus the uniform state (or none). */
function aggregateRec(nodes: DiffNode[]): void {
  for (const n of nodes) {
    if (!n.dir) continue;
    aggregateRec(n.children);
    let add = 0;
    let del = 0;
    let state: string | undefined;
    let uniform = true;
    for (const c of n.children) {
      add += c.dir ? (c.add ?? 0) : (c.file?.add ?? 0);
      del += c.dir ? (c.del ?? 0) : (c.file?.del ?? 0);
      const cs = c.dir ? c.state : c.file?.state;
      if (state === undefined) state = cs;
      if (cs === undefined || cs !== state) uniform = false;
    }
    n.add = add;
    n.del = del;
    n.state = uniform ? state : undefined;
  }
}
