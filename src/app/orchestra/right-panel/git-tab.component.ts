import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from "@angular/core";
import { Agent, Commit, Project } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";
import { fileDir, fileName } from "../utils";
import { CommitFeedComponent } from "./commit-feed.component";

@Component({
  selector: "app-git-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitFeedComponent],
  template: `
    @if (!agent()) {
      <!-- global commit feed -->
      <div class="scroll-y" style="flex:1;padding:8px 0">
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:6px 14px">Commit feed · all worktrees</div>
        <app-commit-feed [commits]="store.commits()" />
      </div>
    } @else {
      @let ag = agent()!;
      <div class="scroll-y" style="flex:1">
        <!-- branch header -->
        <div style="padding:10px 14px;border-bottom:1px solid var(--hair)">
          <div style="display:flex;align-items:center;gap:7px;margin-bottom:4px">
            <app-icon name="branch" size="sm" color="var(--accent-2)" />
            <span style="font-size:11.5px;color:var(--ink)">{{ ag.branch }}</span>
          </div>
          <div class="tnum" style="font-size:10px;color:var(--ink-4);display:flex;gap:8px">
            <span>base {{ ag.base }}</span><span>·</span>
            <span>{{ ag.commits }} ahead</span><span>·</span>
            <span style="color:var(--code-add-ink)">+{{ totAdd() }}</span>
            <span style="color:var(--code-del-ink)">−{{ totDel() }}</span>
          </div>
        </div>

        <!-- working tree status -->
        <div style="padding:10px 14px 6px">
          <div style="display:flex;align-items:center;gap:6px">
            <span class="up" style="font-size:9px;color:var(--ink-3)">{{ staged() ? 'Staged changes' : 'Changes' }}</span>
            <span class="tnum" style="font-size:9px;color:var(--ink-4)">{{ ag.files.length }}</span>
          </div>
        </div>
        @if (ag.files.length) {
          @for (f of ag.files; track f.path) {
            <div style="display:flex;align-items:center;gap:8px;padding:4px 14px;font-size:11px">
              <span [style.color]="stateInk(f.state)" style="flex:none;width:12px;text-align:center;font-size:9px;font-weight:700">{{ f.state }}</span>
              <span [style.color]="staged() ? 'var(--ink-2)' : 'var(--ink)'" [title]="f.path" style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
                <span style="color:var(--ink-4)">{{ fdir(f.path) }}</span>{{ fname(f.path) }}
              </span>
              <span class="tnum" style="flex:none;font-size:9.5px;display:flex;gap:4px">
                <span style="color:var(--code-add-ink)">+{{ f.add }}</span>
                @if (f.del > 0) { <span style="color:var(--code-del-ink)">−{{ f.del }}</span> }
              </span>
            </div>
          }
        } @else {
          <div style="padding:4px 14px 8px;font-size:10.5px;color:var(--ink-4)">clean — no working changes</div>
        }

        <!-- git action buttons -->
        <div style="padding:12px;display:grid;gap:7px">
          @if (ag.files.length > 0) {
            <button class="btn ghost-hair" (click)="staged.set(!staged())" style="justify-content:flex-start">
              <app-icon name="stage" size="sm" />{{ staged() ? 'Unstage all' : 'Stage all changes' }}
            </button>
          }
          <button class="btn ghost-hair" [disabled]="ag.files.length === 0" (click)="store.act(ag.id, 'commit')" style="justify-content:flex-start">
            <app-icon name="commit" size="sm" />Commit {{ staged() ? 'staged' : 'all' }}
          </button>
          <button class="btn ghost-hair" [disabled]="ag.commits === 0" (click)="store.act(ag.id, 'push')" style="justify-content:flex-start">
            <app-icon name="push" size="sm" />Push to origin
          </button>
          <button class="btn ghost-hair" [disabled]="ag.commits === 0" (click)="store.act(ag.id, 'pr')" style="justify-content:flex-start">
            <app-icon name="pr" size="sm" />Open pull request
          </button>
          <button class="btn primary" [disabled]="ag.commits === 0" (click)="store.act(ag.id, 'merge')" style="justify-content:center">
            <app-icon name="merge" size="sm" />Merge {{ ag.branch.replace('agent/', '') }} → {{ project() ? project()!.branch : 'main' }}
          </button>
          <button class="btn ghost-hair" [disabled]="ag.files.length === 0" (click)="store.act(ag.id, 'discard')" style="justify-content:flex-start;color:var(--st-blocked)">
            <app-icon name="discard" size="sm" />Discard working changes
          </button>
        </div>

        <!-- this branch's commits -->
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:4px 14px">Commits on this branch</div>
        <app-commit-feed [commits]="agentCommits()" [compact]="true" />
        @if (!agentCommits().length) {
          <div style="padding:2px 14px 14px;font-size:10.5px;color:var(--ink-4)">no commits yet</div>
        }
      </div>
    }
  `,
})
export class GitTabComponent {
  readonly store = inject(OrchestraStore);
  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);
  readonly staged = signal(false);

  readonly fname = fileName;
  readonly fdir = fileDir;

  readonly totAdd = computed(() => (this.agent()?.files ?? []).reduce((s, f) => s + f.add, 0));
  readonly totDel = computed(() => (this.agent()?.files ?? []).reduce((s, f) => s + f.del, 0));
  readonly agentCommits = computed<Commit[]>(() => {
    const ag = this.agent();
    return ag ? this.store.commits().filter((c) => c.agent === ag.id) : [];
  });

  stateInk(state: string): string {
    return state === "A" ? "var(--code-add-ink)" : state === "D" ? "var(--code-del-ink)" : "var(--accent-2)";
  }
}
