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
import { BlameLine, FileDiff } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { fileDir, fileName, langId } from "../../utils";
import { IconComponent } from "../../shared/icon.component";
import { AddDelComponent } from "../../shared/git/add-del.component";
import { CodeDiffComponent } from "../code-diff.component";
import { KjBadgeComponent, KjButtonComponent } from "@kouji-ui/components";

/**
 * Diff panel for a single file within a commit/range diff view.
 *
 * Always renders `<app-code-diff>`. The "Annotate" toggle overlays a per-line
 * committer gutter on BOTH sides of the diff — the old side blamed at `oldRev`
 * (parent / range-from) and the new side at `newRev` (commit / range-to), since
 * the same line can have different authors on each side. Blame is loaded lazily
 * the first time annotate is turned on for a file.
 */
@Component({
  selector: "app-diff-or-blame",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, AddDelComponent, CodeDiffComponent, KjButtonComponent, KjBadgeComponent],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--bg)">

      <!-- ---- sticky file header ---- -->
      <div class="pane-head" style="position:sticky;top:0;gap:var(--sp-3);padding:var(--sp-3) var(--sp-5);background:var(--panel);z-index:2">
        <app-icon name="file" size="sm" style="color:var(--ink-3)" />
        @if (dirPart()) {
          <span style="color:var(--ink-4)">{{ dirPart() }}</span>
        }
        <span [style.margin-left]="dirPart() ? 'calc(-1 * var(--sp-2))' : null">{{ namePart() }}</span>

        <!-- add/del from the parent via content projection (passed as @Input) -->
        @if (add() !== null && del() !== null) {
          <app-add-del [add]="add() ?? 0" [del]="del() ?? 0" />
        }

        <!-- annotate toggle -->
        <kj-button kjVariant="outline" [kjPressed]="blame()" (click)="blame.set(!blame())" title="Annotate — show who last changed each line on both sides">
          <app-icon size="md" name="git" [color]="blame() ? 'var(--ui-ink)' : null" />
          Annotate
        </kj-button>

        <!-- lang chip -->
        <kj-badge style="font-size:var(--fs-meta)">{{ lang() }}</kj-badge>
      </div>

      <!-- ---- body: the diff, optionally annotated ---- -->
      @if (diff(); as d) {
        <app-code-diff
          style="flex:1;min-height:0"
          [oldText]="d.old"
          [newText]="d.new"
          [lang]="lang()"
          [showBlame]="blame()"
          [oldBlame]="oldBlameData()"
          [newBlame]="newBlameData()"
        />
      } @else {
        <div class="pane-empty">no textual diff</div>
      }

    </div>
  `,
})
export class DiffOrBlameComponent {
  /** Agent id — keys the blame store. */
  readonly agent = input.required<string>();
  /** Repo-relative file path. */
  readonly path = input.required<string>();
  /** The diff data for this file (null while loading or when no textual diff). */
  readonly diff = input<FileDiff | null>(null);

  /** Optional add/del counts to show in the header (null = hidden). */
  readonly add = input<number | null>(null);
  readonly del = input<number | null>(null);

  /** Revision to blame the OLD (left) side at — parent commit / range "from". */
  readonly oldRev = input<string | null>(null);
  /** Revision to blame the NEW (right) side at — the commit / range "to". */
  readonly newRev = input<string | null>(null);

  /** Reserved: clicking a blame cell to open its commit (not wired yet). */
  readonly openCommit = output<string>();

  private readonly store = inject(GitInspectStore);

  /** Annotate (blame) mode toggle — sticky across file switches. */
  readonly blame = signal(false);

  readonly oldBlameData = computed<BlameLine[]>(() => {
    const rev = this.oldRev();
    if (!this.blame() || !rev) return [];
    return this.store.blameFor(this.agent(), this.path(), rev).data;
  });
  readonly newBlameData = computed<BlameLine[]>(() => {
    const rev = this.newRev();
    if (!this.blame() || !rev) return [];
    return this.store.blameFor(this.agent(), this.path(), rev).data;
  });

  constructor() {
    // Lazily load both sides' blame the first time annotate is on for this file.
    // untracked() keeps the store reads/writes out of the effect's deps (the load
    // methods read+write the blame signal — see the freeze fix).
    effect(() => {
      if (!this.blame()) return;
      const id = this.agent();
      const path = this.path();
      const oldRev = this.oldRev();
      const newRev = this.newRev();
      untracked(() => {
        if (id && path && oldRev && this.store.blameFor(id, path, oldRev).status === "idle") {
          this.store.loadBlame(id, path, oldRev);
        }
        if (id && path && newRev && this.store.blameFor(id, path, newRev).status === "idle") {
          this.store.loadBlame(id, path, newRev);
        }
      });
    });
  }

  readonly dirPart = () => fileDir(this.path());
  readonly namePart = () => fileName(this.path());
  readonly lang = () => {
    const d = this.diff();
    return d?.lang || langId(this.path());
  };
}
