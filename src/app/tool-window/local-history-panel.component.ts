import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { CodeDiffComponent } from "../workspace/code-diff.component";
import { HistorySnapshot } from "../data-source/bridge";
import { Agent, FileDiff } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { IconComponent } from "../shared/icon.component";
import { LocalHistoryStore } from "./local-history.store";

/**
 * Bottom-dock "Local History" panel — LIVE since B4.4: the watcher snapshots
 * every settled worktree burst (content-addressed, bounded), this panel lists
 * the timeline, diffs any file against its snapshot content, and restores
 * files (each restore first guard-snapshots the current content, so it is
 * itself undoable).
 */
@Component({
  selector: "app-local-history-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CodeDiffComponent],
  template: `
    @if (!agent()) {
      <div style="padding:var(--sp-8);font-size:var(--fs-sm);color:var(--ink-4)">
        select an agent to see its worktree history
      </div>
    } @else {
      @let ag = agent()!;
      <div style="display:grid;grid-template-columns:280px 1fr;min-height:0;flex:1">
        <!-- snapshots -->
        <div style="display:flex;flex-direction:column;min-height:0;border-right:1px solid var(--hair)">
          <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <app-icon name="clock" size="sm" color="var(--ui-ink)" />
            <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">Snapshots</span>
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-2xs);color:var(--ink-4)">{{ store.snapshots().length }}</span>
            <button class="btn" (click)="refresh()" title="Refresh" style="padding:var(--sp-1);border-radius:4px">
              <app-icon name="refresh" size="sm" [px]="11" />
            </button>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (s of store.snapshots(); track s.id) {
              @let on = sel()?.id === s.id;
              <div
                (click)="select(s)"
                style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-6);cursor:pointer;border-bottom:1px solid var(--hair)"
                [style.background]="on ? 'var(--ui-sel)' : 'transparent'"
              >
                <app-icon [name]="s.trigger === 'before-restore' ? 'discard' : 'clock'" size="sm" [px]="12" [color]="on ? 'var(--ui-ink)' : 'var(--ink-4)'" />
                <div style="flex:1;min-width:0">
                  <div class="tnum" style="font-size:var(--fs-sm)" [style.color]="on ? 'var(--ink)' : 'var(--ink-2)'">{{ when(s.ts) }}</div>
                  <div style="font-size:var(--fs-2xs);color:var(--ink-4)">
                    {{ s.files.length }} file{{ s.files.length === 1 ? '' : 's' }} · {{ s.trigger === 'before-restore' ? 'restore guard' : 'auto' }}
                  </div>
                </div>
              </div>
            }
            @if (!store.snapshots().length) {
              <div style="padding:var(--sp-6);font-size:var(--fs-sm);color:var(--ink-3)">no snapshots yet</div>
              <div style="padding:0 var(--sp-6);font-size:var(--fs-2xs);color:var(--ink-4);line-height:1.5;text-wrap:pretty">
                snapshots are captured automatically whenever files in this worktree change
              </div>
            }
          </div>
        </div>

        <!-- detail -->
        <div style="display:flex;flex-direction:column;min-height:0">
          <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <span style="font-size:var(--fs-sm);color:var(--ink-4)">{{ ag.name }} · {{ ag.branch }}</span>
            <div style="margin-left:auto;display:flex;gap:var(--sp-3);align-items:center">
              @if (confirmRestore()) {
                <span style="font-size:var(--fs-xs);color:var(--ink-3)">restore {{ sel()!.files.length }} file(s) to this point?</span>
                <button class="btn ghost-hair" style="font-size:var(--fs-sm);color:var(--st-blocked)" [disabled]="store.busy()" (click)="doRestoreAll()">Restore</button>
                <button class="btn ghost-hair" style="font-size:var(--fs-sm)" (click)="confirmRestore.set(false)">Cancel</button>
              } @else {
                <button class="btn ghost-hair" [disabled]="!sel() || store.busy()" title="restore every file of this snapshot (a guard snapshot makes this undoable)" style="font-size:var(--fs-sm);color:var(--st-blocked)" (click)="confirmRestore.set(true)">
                  <app-icon name="discard" size="sm" />Revert to this point
                </button>
              }
            </div>
          </div>
          @if (sel(); as s) {
            <div style="display:grid;grid-template-columns:240px 1fr;min-height:0;flex:1">
              <!-- files of the snapshot -->
              <div class="scroll-y" style="border-right:1px solid var(--hair);padding:var(--sp-2) 0">
                @for (f of s.files; track f.path) {
                  @let fon = selFile() === f.path;
                  <div
                    (click)="selectFile(f.path)"
                    style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-6);cursor:pointer"
                    [style.background]="fon ? 'var(--panel-3)' : 'transparent'"
                  >
                    <app-icon name="file" size="sm" [px]="11" [color]="fon ? 'var(--ui-ink)' : 'var(--ink-4)'" />
                    <span style="flex:1;min-width:0;font-size:var(--fs-xs);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [style.color]="fon ? 'var(--ink)' : 'var(--ink-3)'" [title]="f.path">{{ f.path }}</span>
                    <button class="btn lh-op" [disabled]="store.busy()" title="restore only this file" (click)="$event.stopPropagation(); restoreFile(f.path)">
                      <app-icon name="discard" size="sm" [px]="10" />
                    </button>
                  </div>
                }
              </div>
              <!-- snapshot ↔ current diff -->
              <div style="display:flex;flex-direction:column;min-height:0">
                @if (diff(); as d) {
                  <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-6);border-bottom:1px solid var(--hair);flex:none;font-size:var(--fs-2xs);color:var(--ink-4)">
                    <span>snapshot</span><app-icon name="chevron" size="sm" [px]="10" /><span>current</span>
                  </div>
                  <app-code-diff [oldText]="d.old" [newText]="d.new" [lang]="d.lang" />
                } @else if (selFile()) {
                  <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-sm)">loading diff…</div>
                } @else {
                  <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-sm)">select a file to compare snapshot ↔ current</div>
                }
              </div>
            </div>
          } @else {
            <div style="flex:1;display:grid;place-items:center;padding:var(--sp-8)">
              <div style="text-align:center">
                <app-icon name="clock" size="lg" color="var(--hair-2)" />
                <div style="font-size:var(--fs-sm);color:var(--ink-3);margin-top:var(--sp-4)">select a snapshot to inspect or restore</div>
              </div>
            </div>
          }
        </div>
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
      .lh-op {
        display: flex;
        flex: none;
        padding: var(--sp-1);
        border-radius: 4px;
        color: var(--ink-4);
      }
      .lh-op:hover:not(:disabled) {
        color: var(--code-del-ink);
        background: var(--panel-2);
      }
    `,
  ],
})
export class LocalHistoryPanelComponent {
  readonly agent = input<Agent | null>(null);

  readonly store = inject(LocalHistoryStore);
  private agents = inject(AgentsStore);

  readonly sel = signal<HistorySnapshot | null>(null);
  readonly selFile = signal<string | null>(null);
  readonly diff = signal<FileDiff | null>(null);
  readonly confirmRestore = signal(false);
  private diffGen = 0;

  /** Selected snapshot re-resolved against the live list (restores reload it). */
  readonly selLive = computed(() => {
    const id = this.sel()?.id;
    return this.store.snapshots().find((s) => s.id === id) ?? null;
  });

  constructor() {
    effect(() => {
      const ag = this.agent();
      if (ag && this.store.loadedFor() !== ag.id) {
        this.sel.set(null);
        this.selFile.set(null);
        this.diff.set(null);
        void this.store.load(ag.id);
      }
    });
    // new snapshots appear as the watcher captures them while the panel is open
    let unsub: (() => void) | null = null;
    void this.agents
      .onScan((p) => {
        const ag = this.agent();
        if (ag && p.id === ag.id) void this.store.load(ag.id);
      })
      .then((u) => (unsub = u));
    inject(DestroyRef).onDestroy(() => unsub?.());
  }

  when(ts: number): string {
    const d = new Date(ts);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(d.getDate())}/${p(d.getMonth() + 1)} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }

  refresh(): void {
    const ag = this.agent();
    if (ag) void this.store.load(ag.id);
  }

  select(s: HistorySnapshot): void {
    this.sel.set(s);
    this.confirmRestore.set(false);
    this.selFile.set(null);
    this.diff.set(null);
    if (s.files.length === 1) this.selectFile(s.files[0].path);
  }

  selectFile(path: string): void {
    const ag = this.agent();
    const s = this.sel();
    if (!ag || !s) return;
    this.selFile.set(path);
    this.diff.set(null);
    const g = ++this.diffGen;
    void this.store
      .fileDiff(ag.id, s.id, path)
      .then((d) => {
        if (g === this.diffGen) this.diff.set(d);
      })
      .catch(() => {
        if (g === this.diffGen) this.diff.set(null);
      });
  }

  doRestoreAll(): void {
    const ag = this.agent();
    const s = this.sel();
    if (!ag || !s) return;
    this.confirmRestore.set(false);
    void this.store.restore(ag.id, s.id);
  }

  restoreFile(path: string): void {
    const ag = this.agent();
    const s = this.sel();
    if (!ag || !s) return;
    void this.store.restore(ag.id, s.id, [path]);
  }
}
