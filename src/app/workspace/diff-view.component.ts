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
import { fileStateLabel, isMarkdownPath, langId, langTag } from "../utils";
import { BRIDGE, Commands } from "../data-source/bridge";
import { DIFF_LIST_MAX, DIFF_LIST_MIN, UiStore } from "../ui/ui.store";
import { DiffFileListComponent } from "./git/diff-file-list.component";
import { PaneResizerComponent } from "../shared/pane-resizer.component";
import { UnifiedCodeComponent } from "./review/unified-code.component";
import { AnnotateBlameComponent } from "./review/annotate-blame.component";
import { SendReviewButtonComponent } from "./review/send-review.component";
import { DiffStats } from "./review/chunk-stats";
import { KjBadgeComponent, KjButtonComponent } from "@kouji-ui/components";

/** Worktree paths travel with forward slashes; the backend joins them itself. */
const norm = (p: string): string => p.replace(/\\/g, "/");

@Component({
  selector: "app-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, UnifiedCodeComponent, AnnotateBlameComponent, SendReviewButtonComponent, GitActionBarComponent, KjBadgeComponent, KjButtonComponent, DiffFileListComponent, PaneResizerComponent],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;min-width:0">
    <div
      class="diff-grid"
      [style.grid-template-columns]="listW() + 'px 6px 1fr'"
    >
      <!-- file list: THE shared changed-file list — same rows, same collapse,
           same menu as every other diff surface. -->
      <app-diff-file-list
        style="min-height:0;min-width:0"
        [agent]="agent()"
        [files]="changes()"
        [selPath]="current()?.path ?? null"
        title="Changed"
        [allowCreate]="true"
        [canRefresh]="true"
        (select)="select($event)"
        (refresh)="refresh()"
        (mutated)="afterMutation()"
      />

      <!-- resizable separator: drag to rebalance the file list vs the diff body -->
      <app-pane-resizer
        [width]="listW()"
        [min]="LIST_MIN"
        [max]="LIST_MAX"
        (widthChange)="ui.diffListWidth.set($event)"
        (reset)="resetWidth()"
      />

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
  readonly ui = inject(UiStore);
  readonly agent = input.required<Agent>();
  readonly LIST_MIN = DIFF_LIST_MIN;
  readonly LIST_MAX = DIFF_LIST_MAX;

  // selection by PATH (works across both flat + tree views); treeMode toggles
  // them. Selection lives in UiStore keyed by agent — the pane destroys this
  // component on tab switch, so a local signal would forget the opened file.
  readonly selPath = computed(() => this.ui.diffSelectionFor(this.agentId()));
  select(path: string): void {
    this.ui.setDiffSelection(this.agentId(), path);
  }
  /** A worktree write moves BOTH feeds: this list (git status) and the sidebar
   *  file tree, which is rooted at the same agent id. */
  afterMutation(): void {
    const id = this.agentId();
    this.work.loadChanges(id);
    this.work.loadTree(id, true);
  }

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

  // ----- resizable separator -----
  // The width preference lives in UiStore (persisted with the workspace, and
  // shared with every other diff surface), so it survives tab switches AND
  // restarts.
  readonly listW = this.ui.diffListW;

  constructor() {
    // Pin the displayed agent's work data while this view is on screen — a
    // visible key must never be LRU-evicted (eviction blanked the list, the
    // idle-reload below repopulated it, and the cycle flashed forever).
    effect((onCleanup) => {
      onCleanup(this.work.pin(this.agentId()));
    });
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
