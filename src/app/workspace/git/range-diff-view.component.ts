import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
  untracked,
} from "@angular/core";
import { Agent, Commit, CommitFile, CommitsFile, FileDiff, RangeFiles } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { AgentWorkStore } from "../../agents/agent-work.store";
import { UiStore } from "../../ui/ui.store";
import { IconComponent } from "../../shared/icon.component";
import { ShaChipComponent } from "../../shared/git/sha-chip.component";
import { AddDelComponent } from "../../shared/git/add-del.component";
import { DiffSplitComponent } from "./diff-split.component";

/** Unix-seconds → compact "X ago" label. */
function relTime(when: number): string {
  if (!when) return "";
  const sec = Math.max(0, Math.floor(Date.now() / 1000) - when);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d ago`;
  const mo = Math.floor(day / 30);
  if (mo < 12) return `${mo}mo ago`;
  return `${Math.floor(mo / 12)}y ago`;
}

/**
 * Center view for a multi-commit diff. Two semantics share this shell because
 * the chrome — the header, the resizable file list, the lazy per-file diff
 * pane, selection — is identical; only the backend pair and the wording differ:
 *
 *  - `mode="range"`: a two-endpoint TREE compare (oldest selected tree →
 *    newest). Right for "branch A vs branch B", where the endpoints are the
 *    question. Everything committed between them rides along.
 *  - `mode="commits"`: the UNION of what the selected commits THEMSELVES
 *    changed. Right for a ctrl-click selection in the graph: unselected
 *    commits in between contribute nothing, and the oldest selection's own
 *    changes are included (a tree compare would start after them).
 *
 * The per-file diff differs the same way: `range` diffs one from/to pair for
 * every file, while `commits` uses each row's OWN span (`firstSha`/`lastSha`)
 * — two files in one selection need not have been touched by the same commits.
 *
 * The body is `<app-diff-split>` — the same list + diff pane the single-commit
 * and working-tree views use, so a compare collapses, resizes and reads like
 * every other diff in the app.
 */
@Component({
  selector: "app-range-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    ShaChipComponent,
    AddDelComponent,
    DiffSplitComponent,
  ],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">

      <!-- ---- header ----
           What a selection actually needs to say: how much is being compared,
           which two commits bound it, and — on demand — which commits are in
           it. A row of bare sha chips said none of that, and spilled across
           the header as soon as a selection grew past a handful. -->
      <div class="pane-head cmp-head">
        <div class="cmp-title">
          <app-icon name="diff" size="sm" style="color:var(--ui-ink)" />
          <h2>Comparing {{ shas().length }} {{ shas().length === 1 ? 'commit' : 'commits' }}</h2>
          <span class="cmp-stats tnum">
            <span>{{ files().length }} {{ files().length === 1 ? 'file' : 'files' }}</span>
            <app-add-del [add]="totalAdd()" [del]="totalDel()" />
          </span>
        </div>

        <!-- Collapsed: the two commits that bound the compare, oldest on top,
             each with its subject. Expanded: the whole selection instead —
             showing both would just print the endpoints twice. -->
        @if (!expanded()) {
          <div class="cmp-ends">
            <button type="button" class="cmp-end list-row" (click)="openCommit(oldest())" [title]="'Open ' + short(oldest())">
              <app-sha-chip [sha]="oldest()" />
              <span class="trunc">{{ subject(oldest()) }}</span>
              <span class="cmp-when tnum">{{ when(oldest()) }}</span>
            </button>

            @if (newest() !== oldest()) {
              @if (hidden() > 0) {
                <button type="button" class="cmp-more" [attr.aria-expanded]="false" (click)="expanded.set(true)">
                  <app-icon name="chevron" size="sm" />
                  show all {{ shas().length }} commits
                </button>
              } @else {
                <span class="cmp-rule"></span>
              }

              <button type="button" class="cmp-end list-row" (click)="openCommit(newest())" [title]="'Open ' + short(newest())">
                <app-sha-chip [sha]="newest()" />
                <span class="trunc">{{ subject(newest()) }}</span>
                <span class="cmp-when tnum">{{ when(newest()) }}</span>
              </button>
            }
          </div>
        } @else {
          <div class="cmp-ends">
            <button type="button" class="cmp-more" [attr.aria-expanded]="true" (click)="expanded.set(false)">
              <app-icon name="chevronD" size="sm" />
              hide the {{ shas().length }} commits
            </button>
          </div>

          <!-- the whole selection, oldest first — it scrolls, so forty commits
               cost the same header height as four -->
          <div class="cmp-list scroll-y">
            @for (sha of ordered(); track sha) {
              <button type="button" class="cmp-end list-row" (click)="openCommit(sha)" [title]="'Open ' + short(sha)">
                <app-sha-chip [sha]="sha" [dim]="sha !== oldest() && sha !== newest()" />
                <span class="trunc">{{ subject(sha) }}</span>
                <span class="cmp-when tnum">{{ when(sha) }}</span>
              </button>
            }
          </div>
        }

        <!-- Say which of the two semantics is on screen: the two answer the
             same-looking question with different file lists. -->
        <p class="cmp-note">
          @if (mode() === 'commits') {
            The files these commits themselves changed — commits in between are excluded.
          } @else {
            Diffed from the oldest selected commit's tree to the newest's — commits in between are included.
          }
        </p>
      </div>

      <!-- ---- body: the shared file list | diff/blame split ---- -->
      <app-diff-split
        [agent]="agent()"
        [files]="files()"
        title="Files"
        [selPath]="selPath()"
        [diff]="selDiff()"
        [add]="selFile()?.add ?? null"
        [del]="selFile()?.del ?? null"
        [oldRev]="oldRev()"
        [newRev]="newRev()"
        [loading]="filesLoadable().status === 'loading'"
        (select)="onSelect($event)"
      />

    </div>
  `,
  styles: [
    `
      .cmp-head {
        display: block;
        padding-block: var(--sp-3);
        background: var(--panel);
      }
      .cmp-title {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
      }
      .cmp-stats {
        margin-left: auto;
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        font-size: var(--fs-meta);
        color: var(--ink-4);
      }
      .cmp-ends {
        margin-top: var(--sp-2);
        display: flex;
        flex-direction: column;
      }
      /* One row shape for an endpoint and for a row of the expanded list. */
      .cmp-end {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        width: 100%;
        min-width: 0;
        height: var(--row-h);
        padding: 0 var(--sp-3);
        border: 0;
        background: transparent;
        color: var(--ink-2);
        font: inherit;
        font-size: var(--fs-meta);
        text-align: left;
      }
      .cmp-end .trunc {
        flex: 1;
        min-width: 0;
      }
      .cmp-when {
        flex: none;
        color: var(--ink-4);
      }
      /* The link between the two endpoints: what sits between them, and the
         control that reveals it. */
      .cmp-more {
        align-self: flex-start;
        display: flex;
        align-items: center;
        gap: var(--sp-2);
        margin-left: var(--sp-4);
        padding: 0 var(--sp-2);
        border: 0;
        background: transparent;
        color: var(--ink-4);
        font: inherit;
        font-size: var(--fs-meta);
        cursor: pointer;
        border-radius: var(--r-sm);
      }
      .cmp-more:hover {
        color: var(--ink-2);
        background: var(--panel-2);
      }
      .cmp-rule {
        height: var(--sp-3);
        margin-left: calc(var(--sp-4) + 7px);
        border-left: 1px solid var(--hair-2);
      }
      .cmp-list {
        max-height: min(34vh, 220px);
        margin-top: var(--sp-2);
        padding: var(--sp-1) 0;
        border-top: 1px solid var(--hair);
      }
      .cmp-note {
        margin: var(--sp-2) 0 0;
        font-size: var(--fs-meta);
        color: var(--ink-4);
      }
    `,
  ],
})
export class RangeDiffViewComponent {
  /** The owning agent. */
  readonly agent = input.required<Agent>();
  /** Ordered list of commit shas to diff across. */
  readonly shas = input<string[]>([]);
  /** Which semantics to use — see the class doc. Defaults to the endpoint
   *  compare so the branch-vs-branch caller needs no change. */
  readonly mode = input<"range" | "commits">("range");

  private readonly gitStore = inject(GitInspectStore);
  private readonly workStore = inject(AgentWorkStore);
  private readonly ui = inject(UiStore);

  // ---- range mode ----

  /** Loadable from the store for this agent+shas range. */
  private readonly rangeLoadable = computed<{ status: string; data: RangeFiles | null }>(() =>
    this.gitStore.rangeFilesFor(this.agent().id, this.shas())
  );

  /** The RangeFiles result (null until ready). */
  readonly rangeFilesResult = computed<RangeFiles | null>(() =>
    this.mode() === "range" ? this.rangeLoadable().data : null
  );

  // ---- commits mode ----

  private readonly commitsLoadable = computed<{ status: string; data: CommitsFile[] }>(() =>
    this.gitStore.commitsFilesFor(this.agent().id, this.shas())
  );

  // ---- shared ----

  /** The active mode's file-list loadable (drives the "loading…" placeholder). */
  readonly filesLoadable = computed<{ status: string }>(() =>
    this.mode() === "commits" ? this.commitsLoadable() : this.rangeLoadable()
  );

  /** Resolved file list (empty while loading or on a null result). */
  readonly files = computed<CommitFile[]>(() =>
    this.mode() === "commits"
      ? this.commitsLoadable().data
      : this.rangeLoadable().data?.files ?? []
  );

  readonly totalAdd = computed(() => this.files().reduce((s, f) => s + f.add, 0));
  readonly totalDel = computed(() => this.files().reduce((s, f) => s + f.del, 0));

  // ---- the selection, as commits rather than hashes ----

  /** Commit metadata for this agent, by sha — the header's source of subjects
   *  and times. Absent entries degrade to the sha alone. */
  private readonly commitBySha = computed<Map<string, Commit>>(() => {
    const m = new Map<string, Commit>();
    for (const c of this.workStore.commitsFor(this.agent().id).data) m.set(c.sha, c);
    return m;
  });

  /** A selection's shas arrive in graph order; the backend compares them in
   *  COMMIT-TIME order (oldest → newest), so the header sorts the same way and
   *  falls back to the given order when the metadata has not loaded. */
  readonly ordered = computed<string[]>(() => {
    const by = this.commitBySha();
    const shas = [...this.shas()];
    if (!by.size) return shas;
    return shas.sort((a, b) => (by.get(a)?.ts ?? 0) - (by.get(b)?.ts ?? 0));
  });

  /** The endpoints. In range mode the backend has the last word (it resolved
   *  them itself); otherwise the ends of the ordered selection. */
  readonly oldest = computed<string>(
    () => this.rangeFilesResult()?.from ?? this.ordered()[0] ?? ""
  );
  readonly newest = computed<string>(
    () => this.rangeFilesResult()?.to ?? this.ordered()[this.ordered().length - 1] ?? ""
  );

  /** Commits the two endpoint rows don't already show. */
  readonly hidden = computed(() => Math.max(0, this.shas().length - 2));

  /** Whether the full commit list is open under the endpoints. */
  readonly expanded = signal(false);

  /** Currently selected file path. */
  readonly selPath = signal<string | null>(null);

  /** The row for the selected path (add/del counts, and in commits mode the span). */
  readonly selFile = computed<CommitFile | null>(() => {
    const path = this.selPath();
    return this.files().find((f) => f.path === path) ?? null;
  });

  /** Old side of the per-file diff: the range's `from`, or this file's own
   *  oldest touching commit. */
  readonly oldRev = computed<string | null>(() =>
    this.mode() === "commits"
      ? (this.selFile() as CommitsFile | null)?.firstSha ?? null
      : this.rangeFilesResult()?.from ?? null
  );

  /** New side: the range's `to`, or this file's own newest touching commit. */
  readonly newRev = computed<string | null>(() =>
    this.mode() === "commits"
      ? (this.selFile() as CommitsFile | null)?.lastSha ?? null
      : this.rangeFilesResult()?.to ?? null
  );

  /** Loadable diff for the currently selected file. */
  readonly selDiffLoadable = computed<{ status: string; data: FileDiff | null }>(() => {
    const path = this.selPath();
    const id = this.agent().id;
    if (!path || !id) return { status: "idle", data: null };
    if (this.mode() === "commits") {
      const span = this.selFile() as CommitsFile | null;
      if (!span) return { status: "idle", data: null };
      return this.gitStore.commitsFileDiffFor(id, span.firstSha, span.lastSha, path);
    }
    const result = this.rangeFilesResult();
    if (!result) return { status: "idle", data: null };
    return this.gitStore.rangeFileDiffFor(id, result.from, result.to, path);
  });

  readonly selDiff = computed<FileDiff | null>(() => this.selDiffLoadable().data);

  constructor() {
    // Load the file list whenever agent, shas or mode changes.
    // The loaders both READ (…FilesFor) and WRITE (patch) the same store
    // signal. Without untracked(), that read makes the map a dependency of THIS
    // effect and the write then re-invalidates it → infinite effect loop that
    // wedges the main thread. untracked() scopes the deps to agent()/shas()/mode().
    effect(() => {
      const id = this.agent().id;
      const shas = this.shas();
      const mode = this.mode();
      if (!id || !shas.length) return;
      untracked(() =>
        mode === "commits"
          ? this.gitStore.loadCommitsFiles(id, shas)
          : this.gitStore.loadRangeFiles(id, shas)
      );
    });

    // The header names the commits, so it needs their metadata: a compare
    // reached from a persisted git view (rather than from the graph panel) has
    // never loaded the log. A no-op when it is already there.
    effect(() => {
      const id = this.agent().id;
      if (id) untracked(() => this.workStore.ensureCommits(id));
    });

    // Default selection: first file once the list resolves.
    effect(() => {
      const files = this.files();
      if (!files.length) return;
      // Reset selection if the shas changed and old path is no longer in the list.
      if (!this.selPath() || !files.some((f) => f.path === this.selPath())) {
        this.selPath.set(files[0].path);
      }
    });

    // Lazy-load per-file diff on selection change.
    effect(() => {
      const path = this.selPath();
      const id = this.agent().id;
      const old = this.oldRev();
      const next = this.newRev();
      if (!path || !id || !old || !next) return;
      const commits = this.mode() === "commits";
      const loadable = commits
        ? this.gitStore.commitsFileDiffFor(id, old, next, path)
        : this.gitStore.rangeFileDiffFor(id, old, next, path);
      if (loadable.status === "idle") {
        untracked(() =>
          commits
            ? this.gitStore.loadCommitsFileDiff(id, old, next, path)
            : this.gitStore.loadRangeFileDiff(id, old, next, path)
        );
      }
    });
  }

  /** Endpoint shas are full oids from the backend; a label wants chip-length. */
  short(sha: string): string {
    return sha.slice(0, 7);
  }

  /** The commit's subject line — or its sha, when the log hasn't loaded. */
  subject(sha: string): string {
    return this.commitBySha().get(sha)?.msg ?? this.short(sha);
  }

  when(sha: string): string {
    return relTime(this.commitBySha().get(sha)?.ts ?? 0);
  }

  /** A commit in the header is a way INTO that commit: open its own diff. */
  openCommit(sha: string): void {
    if (sha) this.ui.setGitView(this.agent().id, { kind: "commit", sha });
  }

  onSelect(path: string): void {
    this.selPath.set(path);
  }
}
