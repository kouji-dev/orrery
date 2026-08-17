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
import { Agent, Commit, CommitFile, FileDiff } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { AgentWorkStore } from "../../agents/agent-work.store";
import { IconComponent } from "../../shared/icon.component";
import { AuthorAvatarComponent } from "../../shared/git/author-avatar.component";
import { ShaChipComponent } from "../../shared/git/sha-chip.component";
import { DiffFileListComponent } from "./diff-file-list.component";
import { DiffOrBlameComponent } from "./diff-or-blame.component";

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
 * Center view for a single commit diff.
 *
 * Layout:
 *  - CommitContextHeader: commit message + author avatar/name + sha chip + "committed Xago"
 *  - 232px | 1fr grid: DiffFileList (left) + DiffOrBlame (right, lazy per-file load)
 *
 * File list is loaded on `agent`/`sha` change. Per-file diff is loaded lazily on selection.
 */
@Component({
  selector: "app-commit-diff-view",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    AuthorAvatarComponent,
    ShaChipComponent,
    DiffFileListComponent,
    DiffOrBlameComponent,
  ],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">

      <!-- ---- CommitContextHeader ---- -->
      <div style="padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair);background:var(--panel);flex:none">
        <div style="display:flex;align-items:center;gap:var(--sp-3)">
          <app-icon name="commit" size="sm" style="color:var(--ink-3)" />
          <span style="font-size:var(--fs-md);font-weight:600;color:var(--ink);text-wrap:pretty">
            {{ commit()?.msg ?? sha() }}
          </span>
        </div>
        @if (commit(); as c) {
          <div class="tnum" style="margin-top:var(--sp-2);font-size:var(--fs-2xs);color:var(--ink-4);display:flex;align-items:center;gap:var(--sp-3);flex-wrap:wrap">
            <app-author-avatar [author]="c.agent" [size]="15" />
            <span style="color:var(--ink-2)">{{ c.agent }}</span>
            <app-sha-chip [sha]="c.sha" />
            <span>committed {{ relTime(c.ts ?? 0) }}</span>
          </div>
        }
      </div>

      <!-- ---- body: 232px file list | 1fr diff/blame ---- -->
      <div style="flex:1;display:grid;grid-template-columns:232px 1fr;min-height:0">

        <!-- left: changed-files list -->
        <div style="min-height:0;border-right:1px solid var(--hair);background:var(--panel)">
          <app-diff-file-list
            [agent]="agent()"
            [files]="commitFiles()"
            [selPath]="selPath()"
            title="Files in commit"
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
            [oldRev]="sha() + '^'"
            [newRev]="sha()"
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
export class CommitDiffViewComponent {
  /** The owning agent. */
  readonly agent = input.required<Agent>();
  /** Commit sha to display. */
  readonly sha = input.required<string>();
  /** If provided, pre-selects this file path on first load. */
  readonly initPath = input<string | undefined>(undefined);

  private readonly gitStore = inject(GitInspectStore);
  private readonly workStore = inject(AgentWorkStore);

  /** Loadable from the store for this agent+sha's file list. */
  readonly filesLoadable = computed(() =>
    this.gitStore.commitFilesFor(this.agent().id, this.sha())
  );

  /** The resolved file array (empty while loading). */
  readonly commitFiles = computed(() => this.filesLoadable().data);

  /** Commit metadata from AgentWorkStore.commitsFor (matched by sha). */
  readonly commit = computed<Commit | undefined>(() => {
    const sha = this.sha();
    return this.workStore.commitsFor(this.agent().id).data.find((c) => c.sha === sha);
  });

  /** Currently selected file path. */
  readonly selPath = signal<string | null>(null);

  /** The CommitFile entry for the selected path (for add/del counts). */
  readonly selFile = computed<CommitFile | null>(() => {
    const path = this.selPath();
    return this.commitFiles().find((f) => f.path === path) ?? null;
  });

  /** Loadable diff for the currently selected file. */
  readonly selDiffLoadable = computed<{ status: string; data: FileDiff | null }>(() => {
    const path = this.selPath();
    if (!path) return { status: "idle", data: null };
    return this.gitStore.commitFileDiffFor(this.agent().id, this.sha(), path);
  });

  readonly selDiff = computed<FileDiff | null>(() => this.selDiffLoadable().data);

  constructor() {
    // Load file list whenever agent or sha changes.
    // `loadCommitFiles` READS (commitFilesFor) and WRITES (patch) the store's
    // commitFilesMap signal. Without untracked(), the read makes commitFilesMap a
    // dependency of this effect and the write re-invalidates it → infinite effect
    // loop that wedges the main thread. untracked() scopes deps to agent()/sha().
    effect(() => {
      const id = this.agent().id;
      const sha = this.sha();
      if (id && sha) {
        untracked(() => this.gitStore.loadCommitFiles(id, sha));
      }
    });

    // Default selection: initPath if present and in list, else first file.
    // Re-runs when commitFiles resolves (from idle/loading → ready).
    effect(() => {
      const files = this.commitFiles();
      if (!files.length) return;
      const init = this.initPath();
      const candidate = (init && files.some((f) => f.path === init))
        ? init
        : files[0].path;
      // Only set default if nothing is selected yet (or the old selection left this commit).
      if (!this.selPath() || !files.some((f) => f.path === this.selPath())) {
        this.selPath.set(candidate);
      }
    });

    // Lazy-load per-file diff on selection change.
    effect(() => {
      const path = this.selPath();
      const id = this.agent().id;
      const sha = this.sha();
      if (!path || !id || !sha) return;
      const loadable = this.gitStore.commitFileDiffFor(id, sha, path);
      if (loadable.status === "idle") {
        untracked(() => this.gitStore.loadCommitFileDiff(id, sha, path));
      }
    });
  }

  onSelect(path: string): void {
    this.selPath.set(path);
  }

  relTime(ts: number): string {
    return relTime(ts);
  }
}
