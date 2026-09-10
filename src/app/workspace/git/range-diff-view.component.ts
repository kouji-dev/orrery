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
import { Agent, CommitFile, CommitsFile, FileDiff, RangeFiles } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { IconComponent } from "../../shared/icon.component";
import { ShaChipComponent } from "../../shared/git/sha-chip.component";
import { DiffFileListComponent } from "./diff-file-list.component";
import { DiffOrBlameComponent } from "./diff-or-blame.component";

/**
 * Center view for a multi-commit diff. Two semantics share this shell because
 * the chrome — sha chips, 232px file list, lazy per-file diff pane, selection
 * — is identical; only the backend pair and the wording differ:
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
 */
@Component({
  selector: "app-range-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    ShaChipComponent,
    DiffFileListComponent,
    DiffOrBlameComponent,
  ],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">

      <!-- ---- header ---- -->
      <div class="pane-head" style="display:block;padding-block:var(--sp-3);background:var(--panel)">
        <div style="display:flex;align-items:center;gap:var(--sp-3)">
          <app-icon name="diff" size="sm" style="color:var(--ui-ink)" />
          <h2>
            @if (mode() === 'commits') {
              Selected commits · {{ shas().length }}
            } @else {
              Range diff · {{ shas().length }} commits
            }
          </h2>
        </div>
        <div class="tnum" style="margin-top:var(--sp-2);font-size:var(--fs-meta);color:var(--ink-4);display:flex;align-items:center;gap:var(--sp-2);flex-wrap:wrap">
          @for (sha of shas(); track sha; let i = $index) {
            @if (i > 0) {
              <span style="color:var(--ink-4)">·</span>
            }
            <app-sha-chip [sha]="sha" />
          }
        </div>
        <!-- Say which of the two semantics is on screen: the two answer the
             same-looking question with different file lists. -->
        <p style="margin:var(--sp-2) 0 0;font-size:var(--fs-meta);color:var(--ink-4)">
          @if (mode() === 'commits') {
            The files these commits themselves changed — commits in between are excluded.
          } @else if (rangeFilesResult(); as r) {
            Diffed from the oldest selected commit ({{ short(r.from) }}) to the newest ({{ short(r.to) }}) — commits in between are included.
          } @else {
            Diffed from the oldest selected commit's tree to the newest's — commits in between are included.
          }
        </p>
      </div>

      <!-- ---- body: 232px file list | 1fr diff/blame ---- -->
      <div style="flex:1;display:grid;grid-template-columns:232px 1fr;min-height:0">

        <!-- left: aggregated changed-files list -->
        <app-diff-file-list
          style="min-height:0;border-right:1px solid var(--hair);background:var(--panel)"
          [agent]="agent()"
          [files]="files()"
          [selPath]="selPath()"
          [title]="mode() === 'commits' ? 'Selected files' : 'Range files'"
          (select)="onSelect($event)"
        />

        <!-- right: diff or blame -->
        @if (selPath(); as path) {
          <app-diff-or-blame
            [agent]="agent().id"
            [path]="path"
            [diff]="selDiff()"
            [add]="selFile()?.add ?? null"
            [del]="selFile()?.del ?? null"
            [oldRev]="oldRev()"
            [newRev]="newRev()"
          />
        } @else {
          <div class="pane-empty" style="background:var(--bg)">
            @if (filesLoadable().status === 'loading') {
              loading…
            } @else {
              select a file
            }
          </div>
        }

      </div>
    </div>
  `,
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

  /** Endpoint shas are full oids from the backend; the subtitle wants chip-length. */
  short(sha: string): string {
    return sha.slice(0, 7);
  }

  onSelect(path: string): void {
    this.selPath.set(path);
  }
}
