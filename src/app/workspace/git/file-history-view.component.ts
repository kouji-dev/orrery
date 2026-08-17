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
import { Agent, FileHistoryEntry, FileDiff } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { IconComponent } from "../../shared/icon.component";
import { AuthorAvatarComponent } from "../../shared/git/author-avatar.component";
import { ShaChipComponent } from "../../shared/git/sha-chip.component";
import { AddDelComponent } from "../../shared/git/add-del.component";
import { CodeDiffComponent } from "../code-diff.component";
import { fileDir, fileName, langId } from "../../utils";

/** Unix-seconds → human relative label ("2 hours ago", "3 days ago", …). */
function relTime(when: number): string {
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
 * File history center view — Feature 5 from the git-inspection design.
 *
 * Left column: timeline of commits that touched the file (base = A, compare = B).
 * Right column: `<app-code-diff>` showing the file diff at the selected revision.
 *
 * Clicking an unselected revision promotes it to A or B (same logic as the
 * JSX original: whichever end keeps A older than B in list order).
 */
@Component({
  selector: "app-file-history-view",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    AuthorAvatarComponent,
    ShaChipComponent,
    AddDelComponent,
    CodeDiffComponent,
  ],
  template: `
    <div class="fh-grid">

      <!-- ============================================================
           LEFT — revision timeline
           ============================================================ -->
      <div class="fh-left">
        <!-- header -->
        <div class="fh-left-head">
          <div style="display:flex;align-items:center;gap:var(--sp-3)">
            <app-icon name="clock" size="sm" style="color:var(--ink-3)" />
            <span style="color:var(--ink-4);font-size:var(--fs-xs)">{{ dir() }}</span>
            <span style="font-size:var(--fs-sm);color:var(--ink);margin-left:calc(-1 * var(--sp-2))">{{ name() }}</span>
          </div>
          <div style="font-size:var(--fs-2xs);color:var(--ink-3);margin-top:var(--sp-2)">
            {{ revisions().length }} revisions · click two to compare
          </div>
        </div>

        <!-- scroll body -->
        <div class="scroll-y fh-rev-list">
          <!-- timeline connector -->
          <div class="fh-connector"></div>

          @if (loadable().status === 'loading' && !revisions().length) {
            <div class="fh-empty">loading…</div>
          } @else if (loadable().status === 'error') {
            <div class="fh-empty" style="color:var(--st-blocked)">failed to load history</div>
          } @else if (!revisions().length) {
            <div class="fh-empty">no history</div>
          } @else {
            @for (r of revisions(); track r.sha) {
              <div
                class="fh-rev-row"
                [class.fh-rev-base]="r.sha === baseSha()"
                [class.fh-rev-compare]="r.sha === compareSha()"
                (click)="pick(r.sha)"
              >
                <!-- role dot / label -->
                <span class="fh-rev-role">
                  @if (r.sha === baseSha()) {
                    <span class="fh-rev-label fh-rev-label-base up">A</span>
                  } @else if (r.sha === compareSha()) {
                    <span class="fh-rev-label fh-rev-label-compare up">B</span>
                  } @else {
                    <span
                      class="fh-rev-dot"
                      [style.border-color]="'var(--ink-4)'"
                    ></span>
                  }
                </span>

                <!-- content -->
                <div style="flex:1;min-width:0">
                  <div class="fh-rev-msg">{{ r.summary }}</div>
                  <div class="fh-rev-meta tnum">
                    <app-author-avatar [author]="r.author" [size]="15" />
                    <span style="color:var(--ink-3)">{{ r.author }}</span>
                    <app-sha-chip [sha]="r.sha" [dim]="true" />
                    <app-add-del [add]="r.add" [del]="r.del" />
                    <span style="margin-left:auto">{{ rel(r.when) }}</span>
                  </div>
                </div>
              </div>
            }
          }
        </div>
      </div>

      <!-- ============================================================
           RIGHT — diff across revisions
           ============================================================ -->
      <div class="fh-right">
        <!-- diff header -->
        <div class="fh-diff-head">
          <app-icon name="diff" size="sm" style="color:var(--ink-3)" />
          <span style="font-size:var(--fs-xs);color:var(--ink-2)">{{ name() }} across revisions</span>
          <div style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-3)" class="tnum">
            <span class="chip fh-chip-a" style="font-size:var(--fs-2xs);padding:1px var(--sp-3)">A {{ shortSha(baseSha()) }}</span>
            <button class="btn" title="Swap A↔B" (click)="swap()" style="padding:var(--sp-1);border-radius:var(--r-sm)">
              <app-icon name="swap" size="sm" [px]="14" />
            </button>
            <span class="chip fh-chip-b" style="font-size:var(--fs-2xs);padding:1px var(--sp-3)">B {{ shortSha(compareSha()) }}</span>
          </div>
        </div>

        <!-- revision context bar -->
        <div class="fh-rev-bar tnum">
          @if (baseRev(); as a) {
            <span><b style="color:var(--ink)">A</b> {{ rel(a.when) }} · {{ a.summary }}</span>
          }
          @if (compareRev(); as b) {
            <span><b style="color:var(--ink)">B</b> {{ rel(b.when) }} · {{ b.summary }}</span>
          }
        </div>

        <!-- diff body -->
        <div style="flex:1;min-height:0">
          @if (diffLoading()) {
            <div style="height:100%;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-ui)">loading diff…</div>
          } @else if (activeDiff(); as d) {
            @defer (on immediate) {
              <app-code-diff
                style="height:100%"
                [oldText]="d.old"
                [newText]="d.new"
                [lang]="fileLang()"
              />
            } @placeholder {
              <div style="height:100%;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-ui)">loading…</div>
            }
          } @else if (revisions().length < 2) {
            <div style="height:100%;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-ui)">need at least 2 revisions to compare</div>
          } @else {
            <div style="height:100%;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-ui)">select two revisions to compare</div>
          }
        </div>
      </div>

    </div>
  `,
  styles: [
    `
      /* === host: fills the panel, no overflow at this level === */
      :host {
        display: flex;
        flex: 1;
        min-height: 0;
      }

      /* === two-column layout === */
      .fh-grid {
        flex: 1;
        display: grid;
        grid-template-columns: var(--fh-list-w, 346px) 1fr;
        min-height: 0;
        background: var(--panel-2);
      }

      /* === left panel === */
      .fh-left {
        display: flex;
        flex-direction: column;
        min-height: 0;
        border-right: 1px solid var(--hair);
        background: var(--panel);
      }
      .fh-left-head {
        padding: var(--sp-3) var(--sp-5);
        border-bottom: 1px solid var(--hair);
        flex: none;
      }

      /* scroll area — relative so the connector line can be positioned inside */
      .fh-rev-list {
        flex: 1;
        position: relative;
        padding: var(--sp-3) 0;
      }

      /* vertical connector: aligns with the dot/label column */
      .fh-connector {
        position: absolute;
        left: 23px;
        top: var(--sp-4);
        bottom: var(--sp-4);
        width: 1px;
        background: var(--hair);
        pointer-events: none;
      }

      /* === revision row === */
      .fh-rev-row {
        position: relative;
        display: flex;
        gap: var(--sp-3);
        padding: var(--sp-3) var(--sp-4) var(--sp-3) var(--sp-3);
        cursor: pointer;
        border-radius: var(--r-sm);
        margin: 1px var(--sp-3);
        background: transparent;
        transition: background 0.1s ease;
      }
      .fh-rev-row:hover:not(.fh-rev-base):not(.fh-rev-compare) {
        background: var(--panel-2);
      }
      .fh-rev-base {
        background: color-mix(in oklch, var(--side-a), transparent 90%) !important;
        box-shadow: inset 0 0 0 1px var(--side-a);
      }
      .fh-rev-compare {
        background: color-mix(in oklch, var(--side-b), transparent 90%) !important;
        box-shadow: inset 0 0 0 1px var(--side-b);
      }

      /* role column: fixed width so dot/labels align with the connector */
      .fh-rev-role {
        flex: none;
        width: var(--sp-6);
        display: grid;
        place-items: center;
        padding-top: 1px;
      }
      .fh-rev-dot {
        width: var(--sp-3);
        height: var(--sp-3);
        border-radius: 50%;
        background: var(--panel);
        border: 2px solid var(--ink-4);
      }
      .fh-rev-label {
        font-size: var(--fs-2xs);
        font-weight: 700;
      }
      .fh-rev-label-base  { color: var(--side-a); }
      .fh-rev-label-compare { color: var(--side-b); }

      /* commit message + meta row */
      .fh-rev-msg {
        font-size: var(--fs-sm);
        color: var(--ink);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .fh-rev-meta {
        display: flex;
        align-items: center;
        gap: var(--sp-2);
        margin-top: var(--sp-2);
        font-size: var(--fs-2xs);
        color: var(--ink-4);
      }

      /* empty / loading state */
      .fh-empty {
        padding: var(--sp-5) var(--sp-6);
        font-size: var(--fs-sm);
        color: var(--ink-4);
      }

      /* === right panel === */
      .fh-right {
        display: flex;
        flex-direction: column;
        min-height: 0;
        background: var(--bg);
      }
      .fh-diff-head {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-5);
        background: var(--panel);
        border-bottom: 1px solid var(--hair);
        flex: none;
      }

      /* A / B sha chips */
      .fh-chip-a {
        color: var(--ink-2);
        border-color: var(--hair-2);
      }
      .fh-chip-b {
        color: var(--ink-2);
        border-color: var(--hair-2);
      }

      /* revision context bar: shows rel + summary for both ends */
      .fh-rev-bar {
        padding: var(--sp-2) var(--sp-5);
        font-size: var(--fs-2xs);
        color: var(--ink-4);
        border-bottom: 1px solid var(--hair);
        display: flex;
        gap: var(--sp-5);
        flex-wrap: wrap;
        flex: none;
      }
    `,
  ],
})
export class FileHistoryViewComponent {
  private store = inject(GitInspectStore);

  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();

  // ---- derived from inputs ----
  readonly name = computed(() => fileName(this.path()));
  readonly dir  = computed(() => fileDir(this.path()));
  readonly fileLang = computed(() => langId(this.path()));

  // ---- store slice ----
  readonly loadable = computed(() =>
    this.store.fileHistoryFor(this.agent().id, this.path())
  );
  readonly revisions = computed(() => this.loadable().data);

  // ---- comparison selection ----
  /** SHA of the "A" (base / older) end. Default: index 1 (second newest). */
  readonly baseSha = signal<string | null>(null);
  /** SHA of the "B" (compare / newer) end. Default: index 0 (newest). */
  readonly compareSha = signal<string | null>(null);

  // Resolve the actual entry objects for the header context bar
  readonly baseRev = computed<FileHistoryEntry | undefined>(() =>
    this.revisions().find((r) => r.sha === this.baseSha())
  );
  readonly compareRev = computed<FileHistoryEntry | undefined>(() =>
    this.revisions().find((r) => r.sha === this.compareSha())
  );

  // ---- diff state ----
  readonly diffLoading = signal(false);
  readonly activeDiff = signal<FileDiff | null>(null);

  private diffGen = 0;

  constructor() {
    // On path or agent change: load history + reset selection. `loadFileHistory`
    // reads+writes the store's fileHistoryMap signal — untracked() keeps it out of
    // this effect's deps (else the write re-triggers the effect → infinite loop).
    effect(() => {
      const id = this.agent().id;
      const p = this.path();
      untracked(() => this.store.loadFileHistory(id, p));
      this.baseSha.set(null);
      this.compareSha.set(null);
      this.activeDiff.set(null);
    });

    // When revisions load for the first time, seed the default A/B selection
    effect(() => {
      const revs = this.revisions();
      if (revs.length < 2) return;
      // Only seed if selection not yet established
      if (this.baseSha() === null && this.compareSha() === null) {
        this.baseSha.set(revs[1].sha);    // older (index 1) = A
        this.compareSha.set(revs[0].sha); // newer (index 0) = B
      }
    });

    // When A or B selection changes: load the "compare" revision's diff
    // (the diff shows what changed at that commit for this file)
    effect(() => {
      const id   = this.agent().id;
      const p    = this.path();
      const cSha = this.compareSha();
      if (!cSha) {
        this.activeDiff.set(null);
        return;
      }
      const gen = ++this.diffGen;
      this.diffLoading.set(true);
      // loadRevisionDiff synchronously calls loadCommitFileDiff (reads+writes the
      // store's commitFileDiffMap); untracked() keeps that out of this effect's deps.
      untracked(() => void this.loadRevisionDiff(id, cSha, p, gen));
    });
  }

  private async loadRevisionDiff(id: string, sha: string, path: string, gen: number): Promise<void> {
    // Trigger the store load and wait for it to resolve in the map
    this.store.loadCommitFileDiff(id, sha, path);
    // Poll the store until this specific key is ready or errored
    await this.pollUntilReady(id, sha, path, gen);
  }

  private pollUntilReady(id: string, sha: string, path: string, gen: number): Promise<void> {
    return new Promise<void>((resolve) => {
      const check = () => {
        if (gen !== this.diffGen) { resolve(); return; }
        const l = this.store.commitFileDiffFor(id, sha, path);
        if (l.status === "ready" || l.status === "error") {
          if (gen === this.diffGen) {
            this.activeDiff.set(l.status === "ready" ? l.data : null);
            this.diffLoading.set(false);
          }
          resolve();
          return;
        }
        // still idle or loading — check again on next microtask cycle
        setTimeout(check, 50);
      };
      check();
    });
  }

  // ---- interaction ----

  /** Assign the clicked SHA to A or B, preserving A-older-than-B list order. */
  pick(sha: string): void {
    const b = this.baseSha();
    const c = this.compareSha();
    if (sha === b || sha === c) return;

    const revs = this.revisions();
    const idx = (s: string | null) => (s ? revs.findIndex((r) => r.sha === s) : -1);

    // If the new sha is "further down" (older) than the current compare, put it at A
    if (idx(sha) > idx(c)) {
      this.baseSha.set(sha);
    } else {
      this.compareSha.set(sha);
    }
  }

  swap(): void {
    const b = this.baseSha();
    const c = this.compareSha();
    this.baseSha.set(c);
    this.compareSha.set(b);
  }

  // ---- helpers ----
  readonly rel = relTime;

  shortSha(sha: string | null): string {
    return sha ? sha.slice(0, 7) : "—";
  }
}
