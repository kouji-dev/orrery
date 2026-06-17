import {
  ChangeDetectionStrategy,
  Component,
  effect,
  input,
  output,
  signal,
} from "@angular/core";
import { FileDiff } from "../../models";
import { fileDir, fileName, langId } from "../../utils";
import { IconComponent } from "../../shared/icon.component";
import { AddDelComponent } from "../../shared/git/add-del.component";
import { CodeDiffComponent } from "../code-diff.component";
import { FileBlameComponent } from "./file-blame.component";

/**
 * Diff-or-blame panel for a single file within a commit/range diff view.
 *
 * Renders:
 *  - A sticky DiffFileHeader band: dir/name + lang chip + "Annotate" toggle.
 *  - When annotate is OFF → `<app-code-diff>` with the file diff.
 *  - When annotate is ON  → `<app-file-blame>` blame gutter.
 *
 * The annotate toggle resets to OFF whenever `path` changes.
 */
@Component({
  selector: "app-diff-or-blame",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, AddDelComponent, CodeDiffComponent, FileBlameComponent],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--bg)">

      <!-- ---- sticky file header ---- -->
      <div style="position:sticky;top:0;display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-5);background:var(--panel);border-bottom:1px solid var(--hair);font-size:var(--fs-sm);z-index:2;flex:none">
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
        <button
          class="btn"
          [class.ghost-hair]="!blame()"
          (click)="blame.set(!blame())"
          title="Toggle blame / annotate"
          [style.padding]="'var(--sp-1) var(--sp-4)'"
          [style.font-size]="'var(--fs-xs)'"
          [style.color]="blame() ? 'var(--ink)' : 'var(--ink-3)'"
          [style.background]="blame() ? 'color-mix(in oklch, var(--accent), transparent 86%)' : 'transparent'"
          [style.border]="'1px solid ' + (blame() ? 'color-mix(in oklch, var(--accent), transparent 60%)' : 'var(--hair)')"
          style="margin-left:auto;gap:var(--sp-2);border-radius:var(--r-sm)"
        >
          <app-icon name="git" size="sm" [px]="12" [color]="blame() ? 'var(--accent)' : undefined" />
          Annotate
        </button>

        <!-- lang chip -->
        <span class="chip" style="font-size:var(--fs-2xs)">{{ lang() }}</span>
      </div>

      <!-- ---- body: diff or blame ---- -->
      @if (blame()) {
        <app-file-blame
          [agent]="agent()"
          [path]="path()"
          (openCommit)="openCommit.emit($event)"
          style="flex:1;min-height:0"
        />
      } @else {
        @if (diff()) {
          <app-code-diff
            style="flex:1;min-height:0"
            [oldText]="diff()!.old"
            [newText]="diff()!.new"
            [lang]="lang()"
          />
        } @else {
          <div style="display:grid;place-items:center;flex:1;color:var(--ink-4);font-size:var(--fs-sm)">
            no textual diff
          </div>
        }
      }

    </div>
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
    `,
  ],
})
export class DiffOrBlameComponent {
  /** Agent id — forwarded to file-blame. */
  readonly agent = input.required<string>();
  /** Repo-relative file path. */
  readonly path = input.required<string>();
  /** The diff data for this file (null while loading or when no textual diff). */
  readonly diff = input<FileDiff | null>(null);

  /** Optional add/del counts to show in the header (null = hidden). */
  readonly add = input<number | null>(null);
  readonly del = input<number | null>(null);

  /** Emitted when the blame gutter's "open commit" is clicked. */
  readonly openCommit = output<string>();

  /** Annotate (blame) mode toggle — resets to false on path change. */
  readonly blame = signal(false);

  constructor() {
    effect(() => {
      // Reactive on path; reset the toggle whenever the path changes.
      // eslint-disable-next-line @typescript-eslint/no-unused-expressions
      this.path();
      this.blame.set(false);
    });
  }

  readonly dirPart = () => fileDir(this.path());
  readonly namePart = () => fileName(this.path());
  readonly lang = () => {
    const d = this.diff();
    return (d?.lang) || langId(this.path());
  };
}
