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

/** Fixed row height — locked to the lane-SVG geometry (design git-panels.jsx
 *  ROW_H): the svg cell is exactly this tall so lane lines join seamlessly
 *  across rows. Like code surfaces, the graph is not density-scaled. */
const ROW_H = 30;

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
  imports: [IconComponent, AuthorAvatarComponent, ShaChipComponent],
  template: `
    @if (!agent()) {
      <div style="padding:var(--sp-8);font-size:var(--fs-sm);color:var(--ink-4)">
        select a worktree to see its commit graph
      </div>
    } @else {
      @let ag = agent()!;
      <!-- filter row -->
      <div
        style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none"
      >
        <input
          class="gf-input"
          [value]="q()"
          (input)="q.set($any($event.target).value)"
          placeholder="Filter by message or sha…"
          style="flex:1;min-width:90px"
        />
        <input
          class="gf-input"
          disabled
          placeholder="path…"
          title="path filter arrives with B4.1 (needs per-commit file lists)"
          style="width:150px;flex:none;opacity:.55"
        />
        <select class="osel" [value]="author()" (change)="author.set($any($event.target).value)"
          style="width:132px;flex:none;padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)">
          <option value="">All authors</option>
          @for (a of authors(); track a) {
            <option [value]="a">{{ a }}</option>
          }
        </select>
        <select class="osel" [value]="when()" (change)="when.set($any($event.target).value)"
          style="width:108px;flex:none;padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)">
          <option value="all">Any date</option>
          <option value="today">Today</option>
          <option value="week">This week</option>
        </select>
        @if (sel().length === 2) {
          <button class="btn primary" style="flex:none" (click)="openRange()">
            <app-icon name="diff" size="sm" />Compare range
          </button>
        } @else {
          <span style="font-size:var(--fs-3xs);color:var(--ink-4);flex:none">shift-click two commits to diff a range</span>
        }
      </div>

      <!-- rows -->
      <div class="scroll-y" style="flex:1;min-height:0">
        @if (entry().status === 'loading' && !filtered().length) {
          <div style="padding:var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">loading commits…</div>
        } @else if (!filtered().length) {
          <div style="padding:var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">
            {{ commits().length ? "no commits match the filter" : "no commits on this branch yet" }}
          </div>
        }
        @for (c of filtered(); track c.sha; let i = $index; let last = $last) {
          @let on = sel().includes(c.sha);
          <div
            class="graph-row"
            (click)="toggleSel(c.sha, $event.shiftKey)"
            (dblclick)="openCommit(c.sha)"
            [style.background]="on ? 'var(--panel-3)' : ''"
            [style.border-left]="'2px solid ' + (on ? 'var(--accent)' : 'transparent')"
            style="height:30px;display:flex;align-items:center;gap:var(--sp-4);padding:0 var(--sp-6) 0 0;cursor:pointer"
          >
            <!-- single-lane cell: the branch log is a first-parent chain -->
            <svg [attr.width]="29" [attr.height]="rowH" style="flex:none;display:block">
              @if (i > 0) {
                <line x1="11" y1="0" x2="11" [attr.y2]="rowH / 2" stroke="var(--accent)" stroke-width="1.6" stroke-opacity=".75" />
              }
              @if (!last || entry().hasMore) {
                <line x1="11" [attr.y1]="rowH / 2" x2="11" [attr.y2]="rowH" stroke="var(--accent)" stroke-width="1.6" stroke-opacity=".75" />
              }
              <circle cx="11" [attr.cy]="rowH / 2" r="3.6" fill="var(--accent)" />
            </svg>
            @if (i === 0) {
              <span class="chip" style="font-size:var(--fs-3xs);padding:1px var(--sp-3);flex:none;color:var(--st-done);border-color:color-mix(in oklch, var(--st-done), transparent 60%)">
                <app-icon name="branch" size="sm" [px]="10" />{{ ag.branch }}
              </span>
            }
            <span style="flex:1;min-width:0;font-size:var(--fs-sm);color:var(--ink);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ c.msg }}</span>
            <span style="display:flex;align-items:center;gap:var(--sp-2);flex:none;font-size:var(--fs-xs);color:var(--ink-3)">
              <app-author-avatar [author]="c.agent" [size]="15" />{{ c.agent }}
            </span>
            <span class="tnum" style="flex:none;width:56px;font-size:var(--fs-xs);color:var(--ink-4);text-align:right">{{ c.when }}</span>
            <app-sha-chip [sha]="c.sha" />
          </div>
        }
        @if (entry().hasMore) {
          <button
            class="btn ghost-hair"
            style="margin:var(--sp-3) var(--sp-6);justify-content:center;width:calc(100% - var(--sp-9))"
            [disabled]="entry().status === 'loading'"
            (click)="work.loadMoreCommits(ag.id)"
          >{{ entry().status === "loading" ? "loading…" : "Load more" }}</button>
        }
      </div>

      <!-- footer -->
      <div
        class="tnum"
        style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-2) var(--sp-6);border-top:1px solid var(--hair);flex:none;font-size:var(--fs-3xs);color:var(--ink-4)"
      >
        <span>{{ project() ? project()!.name + " · " : "" }}{{ filtered().length }} of {{ commits().length }} commits loaded</span>
        <span style="margin-left:auto">double-click a commit to open its diff</span>
      </div>
    }
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
      }
      .graph-row:hover {
        background: var(--panel-2);
      }
      .gf-input {
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
        padding: var(--sp-2) var(--sp-4);
        color: var(--ink);
        font-family: var(--font-mono);
        font-size: var(--fs-sm);
        outline: none;
      }
    `,
  ],
})
export class CommitGraphPanelComponent {
  readonly work = inject(AgentWorkStore);
  private readonly ui = inject(UiStore);

  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);

  readonly rowH = ROW_H;

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
