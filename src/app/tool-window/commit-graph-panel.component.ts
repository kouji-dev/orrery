import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { Agent, Commit, Project } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { IconComponent } from "../shared/icon.component";
import { AuthorAvatarComponent } from "../shared/git/author-avatar.component";
import { ShaChipComponent } from "../shared/git/sha-chip.component";
import { UiStore } from "../ui/ui.store";
import { rowHeight } from "../ui/density";
import { KjBadgeComponent, KjButtonComponent, KjInputComponent } from "@kouji-ui/components";
import { SelectComponent } from "../shared/select.component";

/* The lane SVG is sized in JS while the row is sized in CSS, and the two MUST
   agree to the pixel or the lane lines break between rows. Both now read the
   same --row-h token, so the graph tracks density like everything else. */

/**
 * Bottom-dock "Git Graph" panel (design git-panels.jsx CommitGraphPanel),
 * wired to the REAL paged branch history of the scoped worktree
 * (AgentWorkStore / agent_commits). The backend log is first-parent linear —
 * the lane cell draws the truthful single-lane chain; multi-lane parent
 * topology arrives with the commit-graph backend (B4.1), as does the per-path
 * filter (needs per-commit file lists up front).
 */
@Component({
  selector: "app-commit-graph-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, AuthorAvatarComponent, ShaChipComponent, KjButtonComponent, KjBadgeComponent, KjInputComponent, SelectComponent],
  template: `
    @if (!agent()) {
      <div class="pane-empty pad">select a worktree to see its commit graph</div>
    } @else {
      @let ag = agent()!;
      <!-- filter row -->
      <div class="pane-head">
        <kj-input
          kjSize="sm"
          [value]="q()"
          (input)="q.set($any($event.target).value)"
          placeholder="Filter by message or sha…"
        />
        <span title="path filter arrives with B4.1 (needs per-commit file lists)" style="width: round(calc(150px * var(--density)), 1px);flex:none;opacity:.55">
          <kj-input kjSize="sm" [disabled]="true" placeholder="path…" />
        </span>
        <app-select [value]="author()" [options]="authorOptions()" (valueChange)="author.set($event)" style="width: round(calc(132px * var(--density)), 1px);flex:none" />
        <app-select [value]="when()" [options]="whenOptions" (valueChange)="when.set($event)" style="width: round(calc(108px * var(--density)), 1px);flex:none" />
        @if (sel().length === 2) {
          <kj-button kjVariant="default" (click)="openRange()">
            <app-icon name="diff" size="sm" />Compare range
          </kj-button>
        } @else {
          <span style="font-size:var(--fs-badge);color:var(--ink-4);flex:none">shift-click two commits to diff a range</span>
        }
      </div>

      <!-- rows -->
      <div class="scroll-y" style="flex:1;min-height:0">
        @if (entry().status === 'loading' && !filtered().length) {
          <div class="pane-empty pad">loading commits…</div>
        } @else if (!filtered().length) {
          <div class="pane-empty pad">
            {{ commits().length ? "no commits match the filter" : "no commits on this branch yet" }}
          </div>
        }
        @for (c of filtered(); track c.sha; let i = $index; let last = $last) {
          @let on = sel().includes(c.sha);
          <div
            class="row-hover"
            (click)="toggleSel(c.sha, $event.shiftKey)"
            (dblclick)="openCommit(c.sha)"
            [style.background]="on ? 'var(--panel-3)' : ''"
            [style.border-left]="'2px solid ' + (on ? 'var(--ui-focus)' : 'transparent')"
            style="height:var(--row-h);display:flex;align-items:center;gap:var(--sp-4);padding:0 var(--sp-6) 0 0;cursor:pointer"
          >
            <!-- single-lane cell: the branch log is a first-parent chain -->
            <svg [attr.width]="29" [attr.height]="rowH()" style="flex:none;display:block">
              @if (i > 0) {
                <line x1="11" y1="0" x2="11" [attr.y2]="rowH() / 2" stroke="var(--lane-1)" stroke-width="1.6" stroke-opacity=".75" />
              }
              @if (!last || entry().hasMore) {
                <line x1="11" [attr.y1]="rowH() / 2" x2="11" [attr.y2]="rowH()" stroke="var(--lane-1)" stroke-width="1.6" stroke-opacity=".75" />
              }
              <circle cx="11" [attr.cy]="rowH() / 2" r="3.6" fill="var(--lane-1)" />
            </svg>
            @if (i === 0) {
              <kj-badge style="font-size:var(--fs-badge);padding:1px var(--sp-3);flex:none;color:var(--st-done);border-color:color-mix(in oklch, var(--st-done), transparent 60%)">
                <app-icon size="sm" name="branch" />{{ ag.branch }}
              </kj-badge>
            }
            <span class="trunc" style="flex:1;color:var(--ink)">{{ c.msg }}</span>
            <span style="display:flex;align-items:center;gap:var(--sp-2);flex:none;color:var(--ink-3)">
              <app-author-avatar [author]="c.agent" [size]="15" />{{ c.agent }}
            </span>
            <span class="tnum" style="flex:none;width:56px;color:var(--ink-4);text-align:right">{{ c.when }}</span>
            <app-sha-chip [sha]="c.sha" />
          </div>
        }
        @if (entry().hasMore) {
          <kj-button kjVariant="toolbar" [kjFullWidth]="true" [kjDisabled]="entry().status === 'loading'" (click)="work.loadMoreCommits(ag.id)">{{ entry().status === "loading" ? "loading…" : "Load more" }}</kj-button>
        }
      </div>

      <!-- footer -->
      <div
        class="tnum"
        style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-2) var(--sp-6);border-top:1px solid var(--hair);flex:none;font-size:var(--fs-badge);color:var(--ink-4)"
      >
        <span>{{ project() ? project()!.name + " · " : "" }}{{ filtered().length }} of {{ commits().length }} commits loaded</span>
        <span style="margin-left:auto">double-click a commit to open its diff</span>
      </div>
    }
  `,
})
export class CommitGraphPanelComponent {
  readonly work = inject(AgentWorkStore);
  private readonly ui = inject(UiStore);

  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);

  /** Lane-cell height. Mirrors the row's CSS height: var(--row-h). Recomputed on
   *  every density switch so the SVG cell and the row stay pixel-identical —
   *  a stale value here shows up as gaps in the lane between commits. */
  readonly rowH = computed(() => {
    void this.ui.tweaks().density;
    return rowHeight();
  });

  readonly q = signal("");
  readonly author = signal("");
  readonly when = signal("all");
  readonly sel = signal<string[]>([]);

  readonly entry = computed(() => {
    const ag = this.agent();
    return this.work.commitsFor(ag ? ag.id : "");
  });
  readonly commits = computed<Commit[]>(() => this.entry().data);
  readonly authors = computed(() => Array.from(new Set(this.commits().map((c) => c.agent))));

  readonly authorOptions = computed(() => [
    { value: "", label: "All authors" },
    ...this.authors().map((a) => ({ value: a, label: a })),
  ]);

  readonly whenOptions = [
    { value: "all", label: "Any date" },
    { value: "today", label: "Today" },
    { value: "week", label: "This week" },
  ];

  readonly filtered = computed<Commit[]>(() => {
    const q = this.q().toLowerCase();
    const author = this.author();
    const when = this.when();
    const now = Date.now() / 1000;
    return this.commits().filter((c) => {
      if (author && c.agent !== author) return false;
      if (q && !((c.msg || "") + " " + c.sha).toLowerCase().includes(q)) return false;
      // date filters need the backend timestamp; commits without one are excluded
      if (when === "today" && !(c.ts && now - c.ts < 86_400)) return false;
      if (when === "week" && !(c.ts && now - c.ts < 7 * 86_400)) return false;
      return true;
    });
  });

  private lastId: string | null = null;
  constructor() {
    // lazy-load the scoped worktree's first commits page; reset ui state on scope change
    effect(() => {
      const ag = this.agent();
      if (!ag) return;
      this.work.ensureCommits(ag.id);
      if (ag.id !== this.lastId) {
        this.lastId = ag.id;
        this.sel.set([]);
      }
    });
  }

  /** Plain click selects one; shift-click accumulates (last two kept). */
  toggleSel(sha: string, shift: boolean): void {
    this.sel.update((s) => {
      if (!shift) return [sha];
      const nx = s.includes(sha) ? s.filter((x) => x !== sha) : [...s, sha];
      return nx.slice(-2);
    });
  }

  openCommit(sha: string): void {
    const ag = this.agent();
    if (ag) this.ui.setGitView(ag.id, { kind: "commit", sha });
  }
  openRange(): void {
    const ag = this.agent();
    if (ag) this.ui.setGitView(ag.id, { kind: "range", shas: this.sel() });
  }
}
