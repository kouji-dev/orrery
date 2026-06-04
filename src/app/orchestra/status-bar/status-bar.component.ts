import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";

@Component({
  selector: "app-status-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <footer style="display:flex;align-items:center;gap:14px;padding:0 14px;background:var(--panel);border-top:1px solid var(--hair);font-size:10.5px;color:var(--ink-3)">
      <span style="display:flex;gap:6px;align-items:center">
        <span class="dot running" style="background:var(--st-running)"></span>{{ running() }} running
      </span>
      @if (blocked() > 0) {
        <span style="display:flex;gap:6px;align-items:center;color:var(--st-blocked)">
          <span class="dot" style="background:var(--st-blocked)"></span>{{ blocked() }} need attention
        </span>
      }
      <span class="tnum">{{ store.projects().length }} projects · {{ store.agents().length }} agents</span>
      <span style="display:flex;gap:5px"><app-icon name="folder" size="sm" [px]="11" />{{ store.worktreeRoot }}</span>
      <span class="tnum" style="margin-left:auto;display:flex;gap:5px;align-items:center">
        @if (store.toast()) {
          <span class="grad-ink" style="font-weight:600">{{ store.toast() }}</span>
          <span style="color:var(--ink-4)">·</span>
        }
        <app-icon name="link" size="sm" [px]="11" />orchestrator: healthy
      </span>
    </footer>
  `,
})
export class StatusBarComponent {
  readonly store = inject(OrchestraStore);
  readonly running = computed(() => this.store.agents().filter((a) => a.status === "running").length);
  readonly blocked = computed(() => this.store.agents().filter((a) => a.status === "blocked").length);
}
