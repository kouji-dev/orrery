import {
  ChangeDetectionStrategy,
  Component,
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
import {
  KjButtonComponent,
  KjConfirmPopupActionComponent,
  KjConfirmPopupActionsComponent,
  KjConfirmPopupCancelComponent,
  KjConfirmPopupComponent,
  KjConfirmPopupContentComponent,
  KjConfirmPopupMessageComponent,
  KjConfirmPopupTriggerComponent,
} from "@kouji-ui/components";

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
  imports: [
    IconComponent,
    CodeDiffComponent,
    KjButtonComponent,
    KjConfirmPopupComponent,
    KjConfirmPopupTriggerComponent,
    KjConfirmPopupContentComponent,
    KjConfirmPopupMessageComponent,
    KjConfirmPopupActionsComponent,
    KjConfirmPopupActionComponent,
    KjConfirmPopupCancelComponent,
  ],
  template: `
    @if (!agent()) {
      <div class="pane-empty pad">select an agent to see its worktree history</div>
    } @else {
      @let ag = agent()!;
      <div style="display:grid;grid-template-columns:280px 1fr;min-height:0;flex:1">
        <!-- snapshots -->
        <div style="display:flex;flex-direction:column;min-height:0;border-right:1px solid var(--hair)">
          <div class="pane-head">
            <app-icon name="clock" size="sm" color="var(--ui-ink)" />
            <span class="up" style="color:var(--ink-3)">Snapshots</span>
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4)">{{ store.snapshots().length }}</span>
            <kj-button kjSize="icon" kjVariant="ghost" (click)="refresh()" title="Refresh">
              <app-icon size="md" name="refresh" />
            </kj-button>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (s of store.snapshots(); track s.id) {
              @let on = sel()?.id === s.id;
              <div
                (click)="select(s)"
                style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-6);cursor:pointer;border-bottom:1px solid var(--hair)"
                [style.background]="on ? 'var(--ui-sel)' : 'transparent'"
              >
                <app-icon size="md" [name]="s.trigger === 'before-restore' ? 'discard' : 'clock'" [color]="on ? 'var(--ui-ink)' : 'var(--ink-4)'" />
                <div style="flex:1;min-width:0">
                  <div class="tnum" [style.color]="on ? 'var(--ink)' : 'var(--ink-2)'">{{ when(s.ts) }}</div>
                  <div style="font-size:var(--fs-meta);color:var(--ink-4)">
                    {{ s.files.length }} file{{ s.files.length === 1 ? '' : 's' }} · {{ s.trigger === 'before-restore' ? 'restore guard' : 'auto' }}
                  </div>
                </div>
              </div>
            }
            @if (!store.snapshots().length) {
              <div style="padding:var(--sp-6);font-size:var(--fs-meta);color:var(--ink-3)">no snapshots yet</div>
              <div style="padding:0 var(--sp-6);font-size:var(--fs-meta);color:var(--ink-4);line-height:1.5;text-wrap:pretty">
                snapshots are captured automatically whenever files in this worktree change
              </div>
            }
          </div>
        </div>

        <!-- detail -->
        <div style="display:flex;flex-direction:column;min-height:0">
          <div class="pane-head">
            <span style="font-size:var(--fs-meta);color:var(--ink-4)">{{ ag.name }} · {{ ag.branch }}</span>
            <div style="margin-left:auto;display:flex;gap:var(--sp-3);align-items:center">
              <kj-confirm-popup [kjDestructive]="true" (kjConfirmed)="doRestoreAll()">
                <kj-confirm-popup-trigger #revertTrig="kjConfirmPopupTrigger">
                  <kj-button kjVariant="danger" [kjDisabled]="!sel() || store.busy()" title="restore every file of this snapshot (a guard snapshot makes this undoable)">
                    <app-icon name="discard" size="sm" />Revert to this point
                  </kj-button>
                </kj-confirm-popup-trigger>
                <kj-confirm-popup-content [kjFor]="revertTrig">
                  <kj-confirm-popup-message>restore {{ sel()?.files?.length ?? 0 }} file(s) to this point?</kj-confirm-popup-message>
                  <kj-confirm-popup-actions>
                    <kj-confirm-popup-cancel><kj-button kjVariant="toolbar">Cancel</kj-button></kj-confirm-popup-cancel>
                    <kj-confirm-popup-action><kj-button kjVariant="danger" [kjDisabled]="store.busy()">Restore</kj-button></kj-confirm-popup-action>
                  </kj-confirm-popup-actions>
                </kj-confirm-popup-content>
              </kj-confirm-popup>
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
                    <app-icon size="md" name="file" [color]="fon ? 'var(--ui-ink)' : 'var(--ink-4)'" />
                    <span class="trunc" style="flex:1" [style.color]="fon ? 'var(--ink)' : 'var(--ink-3)'" [title]="f.path">{{ f.path }}</span>
                    <kj-button kjSize="icon" kjVariant="toolbar" [kjDisabled]="store.busy()" title="restore only this file" (click)="$event.stopPropagation(); restoreFile(f.path)">
                      <app-icon size="sm" name="discard" />
                    </kj-button>
                  </div>
                }
              </div>
              <!-- snapshot ↔ current diff -->
              <div style="display:flex;flex-direction:column;min-height:0">
                @if (diff(); as d) {
                  <div class="pane-head" style="font-size:var(--fs-meta);color:var(--ink-4)">
                    <span>snapshot</span><app-icon size="sm" name="chevron" /><span>current</span>
                  </div>
                  <app-code-diff [oldText]="d.old" [newText]="d.new" [lang]="d.lang" />
                } @else if (selFile()) {
                  <div class="pane-empty">loading diff…</div>
                } @else {
                  <div class="pane-empty">select a file to compare snapshot ↔ current</div>
                }
              </div>
            </div>
          } @else {
            <div class="pane-empty pad">
              <div style="text-align:center">
                <app-icon name="clock" size="lg" color="var(--hair-2)" />
                <div style="font-size:var(--fs-meta);color:var(--ink-3);margin-top:var(--sp-4)">select a snapshot to inspect or restore</div>
              </div>
            </div>
          }
        </div>
      </div>
    }
  `,
})
export class LocalHistoryPanelComponent {
  readonly agent = input<Agent | null>(null);

  readonly store = inject(LocalHistoryStore);
  private agents = inject(AgentsStore);

  readonly sel = signal<HistorySnapshot | null>(null);
  readonly selFile = signal<string | null>(null);
  readonly diff = signal<FileDiff | null>(null);
  private diffGen = 0;

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
    void this.store.restore(ag.id, s.id);
  }

  restoreFile(path: string): void {
    const ag = this.agent();
    const s = this.sel();
    if (!ag || !s) return;
    void this.store.restore(ag.id, s.id, [path]);
  }
}
