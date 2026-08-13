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
import { Agent, CommitFile, FileDiff, RangeFiles } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { IconComponent } from "../../shared/icon.component";
import { ShaChipComponent } from "../../shared/git/sha-chip.component";
import { DiffFileListComponent } from "./diff-file-list.component";
import { DiffOrBlameComponent } from "./diff-or-blame.component";

/**
 * Center view for a range diff (N selected commits).
 *
 * Layout:
 *  - Header: "Range diff · N commits" + sha chips for each selected sha
 *  - 232px | 1fr grid: DiffFileList (aggregated range files) + DiffOrBlame (lazy per-file)
 *
 * Uses `rangeFilesFor`/`loadRangeFiles` for the file list and
 * `rangeFileDiffFor`/`loadRangeFileDiff` for per-file diffs, keyed by from/to
 * from the RangeFiles result.
 */
@Component({
  selector: "app-range-diff-view",
  standalone: true,
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
      <div style="padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair);background:var(--panel);flex:none">
        <div style="display:flex;align-items:center;gap:var(--sp-3)">
          <app-icon name="diff" size="sm" style="color:var(--accent)" />
          <span style="font-size:var(--fs-md);font-weight:600;color:var(--ink)">
            Range diff · {{ shas().length }} commits
          </span>
        </div>
        <div class="tnum" style="margin-top:var(--sp-2);font-size:var(--fs-2xs);color:var(--ink-4);display:flex;align-items:center;gap:var(--sp-2);flex-wrap:wrap">
          @for (sha of shas(); track sha; let i = $index) {
            @if (i > 0) {
              <span style="color:var(--ink-4)">·</span>
            }
            <app-sha-chip [sha]="sha" />
          }
        </div>
      </div>

      <!-- ---- body: 232px file list | 1fr diff/blame ---- -->
      <div style="flex:1;display:grid;grid-template-columns:232px 1fr;min-height:0">

        <!-- left: aggregated changed-files list -->
        <div style="min-height:0;border-right:1px solid var(--hair);background:var(--panel)">
          <app-diff-file-list
            [agent]="agent()"
            [files]="rangeFiles()"
            [selPath]="selPath()"
            title="Range files"
            (select)="onSelect($event)"
          />
        </div>

        <!-- right: diff or blame -->
        @if (selPath(); as path) {
          <app-diff-or-blame
            [agent]="agent().id"
            [path]="path"
            [diff]="selDiff()"
            [add]="selFile()?.add ?? null"
            [del]="selFile()?.del ?? null"
            [oldRev]="rangeFilesResult()?.from ?? null"
            [newRev]="rangeFilesResult()?.to ?? null"
          />
        } @else {
          <div style="display:grid;place-items:center;color:var(--ink-4);background:var(--bg);font-size:var(--fs-sm)">
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
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
      }
    `,
  ],
})
export class RangeDiffViewComponent {
  /** The owning agent. */
  readonly agent = input.required<Agent>();
  /** Ordered list of commit shas to range-diff across. */
  readonly shas = input<string[]>([]);

  private readonly gitStore = inject(GitInspectStore);

  /** Loadable from the store for this agent+shas range. */
  readonly filesLoadable = computed<{ status: string; data: RangeFiles | null }>(() =>
    this.gitStore.rangeFilesFor(this.agent().id, this.shas())
  );

  /** The RangeFiles result (null until ready). */
  readonly rangeFilesResult = computed<RangeFiles | null>(() => this.filesLoadable().data);

  /** Resolved file list (empty array while loading or null result). */
  readonly rangeFiles = computed<CommitFile[]>(() =>
    this.rangeFilesResult()?.files ?? []
  );

  /** Currently selected file path. */
  readonly selPath = signal<string | null>(null);

  /** The CommitFile entry for the selected path (for add/del counts). */
  readonly selFile = computed<CommitFile | null>(() => {
    const path = this.selPath();
    return this.rangeFiles().find((f) => f.path === path) ?? null;
  });

  /** Loadable diff for the currently selected file. */
  readonly selDiffLoadable = computed<{ status: string; data: FileDiff | null }>(() => {
    const path = this.selPath();
    const result = this.rangeFilesResult();
    if (!path || !result) return { status: "idle", data: null };
    return this.gitStore.rangeFileDiffFor(this.agent().id, result.from, result.to, path);
  });

  readonly selDiff = computed<FileDiff | null>(() => this.selDiffLoadable().data);

  constructor() {
    // Load range files whenever agent or shas changes.
    // `loadRangeFiles` both READS (rangeFilesFor) and WRITES (patch) the store's
    // rangeFilesMap signal. Without untracked(), that read makes rangeFilesMap a
    // dependency of THIS effect and the write then re-invalidates it → infinite
    // effect loop that wedges the main thread. untracked() scopes the effect's
    // deps to agent()/shas() only.
    effect(() => {
      const id = this.agent().id;
      const shas = this.shas();
      if (id && shas.length) {
        untracked(() => this.gitStore.loadRangeFiles(id, shas));
      }
    });

    // Default selection: first file once rangeFiles resolves.
    effect(() => {
      const files = this.rangeFiles();
      if (!files.length) return;
      // Reset selection if the shas changed and old path is no longer in the list.
      if (!this.selPath() || !files.some((f) => f.path === this.selPath())) {
        this.selPath.set(files[0].path);
      }
    });

    // Lazy-load per-file diff on selection change.
    effect(() => {
      const path = this.selPath();
      const result = this.rangeFilesResult();
      const id = this.agent().id;
      if (!path || !result || !id) return;
      const loadable = this.gitStore.rangeFileDiffFor(id, result.from, result.to, path);
      if (loadable.status === "idle") {
        untracked(() => this.gitStore.loadRangeFileDiff(id, result.from, result.to, path));
      }
    });
  }

  onSelect(path: string): void {
    this.selPath.set(path);
  }
}
