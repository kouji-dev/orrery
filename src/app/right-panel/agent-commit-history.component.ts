import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  untracked,
} from "@angular/core";
import { Agent, Commit, CommitFile } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { GitInspectStore } from "../agents/git-inspect.store";
import { IconComponent } from "../shared/icon.component";
import { AuthorAvatarComponent } from "../shared/git/author-avatar.component";
import { ShaChipComponent } from "../shared/git/sha-chip.component";
import { StateBadgeComponent } from "../shared/git/state-badge.component";
import { AddDelComponent } from "../shared/git/add-del.component";
import { fileName } from "../utils";

@Component({
  selector: "app-agent-commit-history",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    AuthorAvatarComponent,
    ShaChipComponent,
    StateBadgeComponent,
    AddDelComponent,
  ],
  template: `
    <!-- section header -->
    <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-6)">
      <span class="up" style="font-size:var(--fs-2xs);color:var(--ink-3)">Commits on this branch</span>
      <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)">{{ commits().length }}</span>
    </div>

    <!-- selection range bar -->
    @if (selecting()) {
      <div class="rise" style="display:flex;align-items:center;gap:var(--sp-3);margin:var(--sp-1) var(--sp-4) var(--sp-4);padding:var(--sp-3) var(--sp-5);border-radius:var(--r-sm);background:color-mix(in oklch,var(--accent),transparent 90%);border:1px solid color-mix(in oklch,var(--accent),transparent 70%)">
        <span style="font-size:var(--fs-sm);color:var(--ink)"><b class="tnum">{{ sel().length }}</b> selected</span>
        <button class="btn" (click)="clearSel()" style="margin-left:auto;padding:var(--sp-1) var(--sp-4);font-size:var(--fs-xs);color:var(--ink-3)">Clear</button>
        <button class="btn primary" (click)="emitRange()" style="padding:var(--sp-1) var(--sp-5);font-size:var(--fs-sm)">
          <app-icon name="diff" size="sm" />Diff ({{ sel().length }})
        </button>
      </div>
    }

    <!-- commit rows — capped + scrollable on their own, so the section header
         above (and Load more below) stay put however long the branch gets -->
    <div class="scroll-y ach-list">
    @for (c of commits(); track c.sha; let i = $index) {
      @let isHead = i === 0;
      @let expanded = isExpanded(c.sha);
      @let selected = sel().includes(c.sha);
      <div
        style="margin:0 var(--sp-3);border-radius:var(--r-sm)"
        [style.background]="selected ? 'color-mix(in oklch,var(--accent),transparent 90%)' : 'transparent'"
      >
        <!-- row header -->
        <div
          class="commit-row-inner"
          (click)="toggleExpand(c.sha)"
          style="position:relative;display:flex;gap:var(--sp-4);padding:var(--sp-3) var(--sp-4);cursor:pointer"
        >
          <!-- select checkbox -->
          <button
            (click)="$event.stopPropagation(); toggleSel(c.sha)"
            title="Select for range diff"
            [style.border]="'1px solid ' + (selected ? 'var(--accent)' : 'var(--hair-2)')"
            [style.background]="selected ? 'var(--accent)' : 'transparent'"
            [style.opacity]="selecting() || selected ? '1' : '0.5'"
            style="flex:none;width:var(--sp-6);height:var(--sp-6);margin-top:1px;border-radius:4px;display:grid;place-items:center;cursor:pointer;padding:0"
          >
            @if (selected) { <app-icon name="check" size="sm" [px]="10" color="#06070b" /> }
          </button>

          <!-- main content -->
          <div style="flex:1;min-width:0">
            <!-- top row: chevron + msg + HEAD chip -->
            <div style="display:flex;align-items:center;gap:var(--sp-3)">
              <app-icon
                [name]="expanded ? 'chevronD' : 'chevron'"
                size="sm"
                [px]="11"
                color="var(--ink-4)"
              />
              <span
                [title]="c.msg"
                style="font-size:var(--fs-sm);color:var(--ink);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0"
              >{{ c.msg }}</span>
              @if (isHead) {
                <span class="chip tnum" style="font-size:var(--fs-2xs);padding:0 var(--sp-3);color:var(--accent-2);flex:none">HEAD</span>
              }
            </div>

            <!-- bottom row: avatar + sha + file-count + add/del + rel-time -->
            @let tot = totalsBySha()[c.sha];
            <div class="tnum" style="display:flex;align-items:center;gap:var(--sp-3);margin-top:var(--sp-2);padding-left:var(--sp-6);font-size:var(--fs-2xs);color:var(--ink-4)">
              <app-author-avatar [author]="c.agent" [size]="13" />
              <app-sha-chip [sha]="c.sha" [dim]="true" />
              <span>{{ c.files }}f</span>
              <app-add-del [add]="tot?.add ?? 0" [del]="tot?.del ?? 0" />
              <span style="margin-left:auto">{{ c.when }}</span>
            </div>
          </div>
        </div>

        <!-- expanded file list -->
        @if (expanded) {
          @let cfLoad = commitFiles(c.sha);
          <div style="padding-left:var(--sp-9);padding-bottom:var(--sp-2)">
            @if (cfLoad.status === 'loading' && !cfLoad.data.length) {
              <div style="font-size:var(--fs-xs);color:var(--ink-4);padding:var(--sp-2) var(--sp-4)">loading…</div>
            } @else if (cfLoad.status === 'error') {
              <div style="font-size:var(--fs-xs);color:var(--st-blocked);padding:var(--sp-2) var(--sp-4)">failed to load files</div>
            } @else {
              @for (f of cfLoad.data; track f.path) {
                <div
                  class="cf-row"
                  (click)="openCommit.emit({ sha: c.sha, path: f.path })"
                  style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-1) var(--sp-4) var(--sp-1) var(--sp-2);cursor:pointer;border-radius:var(--r-sm)"
                >
                  <app-state-badge [state]="f.state" />
                  <span
                    [title]="f.path"
                    style="flex:1;font-size:var(--fs-xs);color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
                  >{{ fname(f.path) }}</span>
                  <app-add-del [add]="f.add" [del]="f.del" />
                  <button
                    class="pane-btn"
                    title="File history"
                    (click)="$event.stopPropagation(); openFileHistory.emit(f.path)"
                    style="flex:none"
                  >
                    <app-icon name="clock" size="sm" [px]="12" />
                  </button>
                </div>
              }
            }
          </div>
        }
      </div>
    }
    </div>

    <!-- load-more / empty states -->
    @if (commitsEntry()?.status === 'loading' && !commits().length) {
      <div style="padding:var(--sp-2) var(--sp-6) var(--sp-6);font-size:var(--fs-xs);color:var(--ink-4)">loading commits…</div>
    } @else if (!commits().length) {
      <div style="padding:var(--sp-2) var(--sp-6) var(--sp-6);font-size:var(--fs-xs);color:var(--ink-4)">no commits yet</div>
    }
    @if (commitsEntry()?.hasMore) {
      <button
        class="btn ghost-hair"
        style="margin:var(--sp-2) var(--sp-6) var(--sp-6);justify-content:center;width:calc(100% - var(--sp-9))"
        [disabled]="commitsEntry()?.status === 'loading'"
        (click)="work.loadMoreCommits(agent().id)"
      >{{ commitsEntry()?.status === 'loading' ? 'loading…' : 'Load more' }}</button>
    }
  `,
  styles: [`
    .commit-row-inner:hover { background: var(--panel-2); border-radius: var(--r-sm); }
    .cf-row:hover { background: var(--panel-2); }
    /* ~7 two-line commit rows, then scroll — token-derived for density */
    .ach-list { max-height: calc(var(--sp-9) * 14); overflow-y: auto; }
  `],
})
export class AgentCommitHistoryComponent {
  readonly work = inject(AgentWorkStore);
  private readonly gitInspect = inject(GitInspectStore);

  readonly agent = input.required<Agent>();

  /** Emits when user clicks a file row inside an expanded commit. */
  readonly openCommit = output<{ sha: string; path?: string }>();
  /** Emits the selected sha array when user clicks "Diff (N)". */
  readonly openRange = output<string[]>();
  /** Emits the file path when user clicks the clock button on a file row. */
  readonly openFileHistory = output<string>();

  // ---- local ui state ----
  // Set of expanded commit shas — every commit toggles independently. (Was a
  // single `expandedSha`, which only allowed one open at a time AND fought manual
  // collapse: the auto-expand effect re-opened HEAD the instant it went null.)
  readonly expanded = signal<Set<string>>(new Set());
  private didAutoExpand = false;
  readonly sel = signal<string[]>([]);

  isExpanded(sha: string): boolean {
    return this.expanded().has(sha);
  }

  readonly fname = fileName;

  // ---- derived ----
  readonly commitsEntry = computed(() => this.work.commitsFor(this.agent().id));
  readonly commits = computed<Commit[]>(() => this.commitsEntry().data);
  readonly selecting = computed(() => this.sel().length > 0);

  constructor() {
    // Ensure commits are loaded when this component is first rendered.
    effect(() => {
      const id = this.agent().id;
      // Reading agent() registers the dependency; then ensure:
      this.work.ensureCommits(id);
    });

    // Auto-expand the HEAD commit ONCE on first load. A plain flag (not a signal
    // read) gates it so collapsing HEAD afterwards is NOT undone by this effect.
    effect(() => {
      const cs = this.commits();
      if (cs.length && !this.didAutoExpand) {
        this.didAutoExpand = true;
        this.expanded.set(new Set([cs[0].sha]));
        untracked(() => this.loadFilesIfNeeded(cs[0].sha));
      }
    });
  }

  // ---- commit file helpers ----
  commitFiles(sha: string) {
    return this.gitInspect.commitFilesFor(this.agent().id, sha);
  }

  private loadFilesIfNeeded(sha: string): void {
    const entry = this.gitInspect.commitFilesFor(this.agent().id, sha);
    if (entry.status === "idle") {
      this.gitInspect.loadCommitFiles(this.agent().id, sha);
    }
  }

  /**
   * add/del totals per commit sha, derived from the lazily-loaded file lists
   * (0 until a commit is expanded and its files load). Memoized as a single
   * computed so it recomputes only when commit-file data changes — not once per
   * row on every change-detection pass, which the old template methods did.
   */
  readonly totalsBySha = computed<Record<string, { add: number; del: number }>>(() => {
    const id = this.agent().id;
    const out: Record<string, { add: number; del: number }> = {};
    for (const c of this.commits()) {
      let add = 0;
      let del = 0;
      for (const f of this.gitInspect.commitFilesFor(id, c.sha).data) {
        add += f.add;
        del += f.del;
      }
      out[c.sha] = { add, del };
    }
    return out;
  });

  // ---- interaction ----
  toggleExpand(sha: string): void {
    this.expanded.update((s) => {
      const next = new Set(s);
      if (next.has(sha)) {
        next.delete(sha);
      } else {
        next.add(sha);
        this.loadFilesIfNeeded(sha);
      }
      return next;
    });
  }

  toggleSel(sha: string): void {
    this.sel.update((s) =>
      s.includes(sha) ? s.filter((x) => x !== sha) : [...s, sha]
    );
  }

  clearSel(): void {
    this.sel.set([]);
  }

  emitRange(): void {
    const sel = this.sel();
    // A single selected commit can't be a meaningful range (diffing it against
    // itself is empty) — show that commit's own changes (vs its parent) instead.
    if (sel.length === 1) {
      this.openCommit.emit({ sha: sel[0] });
    } else {
      this.openRange.emit(sel);
    }
  }
}
