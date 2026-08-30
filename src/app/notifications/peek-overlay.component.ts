import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  OnDestroy,
  OnInit,
  signal,
} from "@angular/core";
import { ActivityKind, AgentNotification } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentsStore } from "../stores/agents.store";
import { CommandRegistryService } from "../commands/command-registry.service";
import { NotificationService } from "./notification.service";
import { NotificationCardComponent } from "./notification-card.component";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";

/**
 * v2 peek overlay — the unblock queue (design v2.jsx PeekOverlay). Opens OVER
 * the workspace, never instead of it: the scrim is deliberately light so the
 * user can still see what they were doing. Walks every pending notification
 * (oldest first) with N / ⇧N; shows the last few PTY context lines above the
 * request; permission prompts resolve via the shared notification card; a
 * reply line writes straight into the agent's PTY.
 */
@Component({
  selector: "app-peek-overlay",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, ToolBadgeComponent, NotificationCardComponent],
  template: `
    @let cur = current();
    <!-- light scrim — the workspace stays legible behind the queue -->
    <div
      (mousedown)="registry.close()"
      style="position:fixed;inset:0;z-index:80;background:color-mix(in oklch, var(--scrim), transparent 28%)"
    ></div>
    <div
      class="surface rise"
      role="dialog"
      aria-label="Needs you — unblock queue"
      style="position:fixed;left:50%;top:84px;transform:translateX(-50%);width:min(720px, 92vw);max-height:70vh;display:flex;flex-direction:column;z-index:81;box-shadow:var(--shadow);overflow:hidden"
    >
      <!-- header -->
      <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
        <app-icon name="bell" size="sm" color="var(--ui-ink)" />
        <span class="up" style="font-size:var(--fs-2xs);color:var(--ink-3)">Needs you</span>
        <span class="chip tnum" style="font-size:var(--fs-3xs)">{{ items().length ? (index() + 1) + ' of ' + items().length : '0 pending' }}</span>
        <span style="margin-left:auto;display:inline-flex;align-items:center;gap:var(--sp-3);font-size:var(--fs-2xs);color:var(--ink-4)">
          <span class="kbd">⇧N</span>prev<span class="kbd">N</span>next
        </span>
        <button class="pane-btn" (click)="registry.close()"><app-icon name="x" size="sm" /></button>
      </div>

      <!-- body -->
      <div class="scroll-y" style="flex:1;min-height:0;padding:var(--sp-5) var(--sp-6);display:flex;flex-direction:column;gap:var(--sp-4)">
        @if (cur) {
          @let ag = agentOf(cur);
          <!-- agent identity -->
          <div style="display:flex;align-items:center;gap:var(--sp-4);flex-wrap:wrap">
            <app-status-dot [status]="ag?.status ?? 'idle'" />
            <span style="font-size:var(--fs-ui);font-weight:500">{{ cur.agentName }}</span>
            @if (ag) { <app-tool-badge [tool]="ag.tool" [size]="14" /> }
            @if (projectOf(cur); as proj) {
              <span class="chip" [style.color]="proj.color" [style.border-color]="mix(proj.color, 65)" style="font-size:var(--fs-3xs);padding:1px var(--sp-3)">{{ proj.name }}</span>
            }
            @if (ag) {
              <span style="display:inline-flex;align-items:center;gap:var(--sp-2);font-size:var(--fs-2xs);color:var(--ink-3)">
                <app-icon name="branch" size="sm" />{{ ag.branch }}
              </span>
            }
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-2xs);color:var(--ink-4)">{{ ago(cur.createdAt) }}</span>
          </div>

          <!-- last PTY lines — enough context to judge without leaving -->
          @if (context().length) {
            <div class="mono" style="background:var(--bg);border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-3) var(--sp-5);font-size:var(--fs-2xs);line-height:1.75;color:var(--ink-3)">
              @for (l of context(); track $index) {
                <div style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">
                  <span style="display:inline-block;width:round(calc(12px * var(--density)), 1px);color:var(--ink-4)">{{ prefix(l.kind) }}</span>{{ l.text }}
                </div>
              }
            </div>
          }

          <!-- the request itself — same card as the bell feed, same actions.
               An action that NAVIGATES (open terminal / review diff) closes
               the queue — the user chose to leave it. -->
          <div style="border:1px solid var(--hair);border-radius:var(--r-md);overflow:hidden">
            <app-notification-card [notification]="cur" (navigate)="registry.close()" />
          </div>

          <!-- reply straight into the PTY -->
          <div style="display:flex;align-items:center;gap:var(--sp-4);height:var(--ctl-h-lg);padding:0 var(--sp-5);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);flex:none">
            <app-icon name="terminal" size="sm" color="var(--ink-4)" />
            <input
              [value]="reply()"
              (input)="reply.set($any($event.target).value)"
              (keydown.enter)="send()"
              [placeholder]="'…or type a reply — writes straight to ' + cur.agentName + '\\u2019s PTY'"
              style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm)"
            />
            <span class="kbd">↵</span>
          </div>
        } @else {
          <div style="padding:var(--sp-9) var(--sp-4);display:flex;flex-direction:column;align-items:center;gap:var(--sp-3);text-align:center">
            <span class="disp" style="font-size:var(--fs-lg);font-weight:500;color:var(--ink)">Inbox zero</span>
            <span style="font-size:var(--fs-sm);color:var(--ink-3)">Nothing pending — agents are running quietly.</span>
          </div>
        }
      </div>

      <!-- footer key hints -->
      <div style="display:flex;align-items:center;gap:var(--sp-6);padding:var(--sp-3) var(--sp-6);border-top:1px solid var(--hair);background:var(--panel-2);flex:none;font-size:var(--fs-2xs);color:var(--ink-3);white-space:nowrap;overflow:hidden">
        <span><span class="kbd">↵</span> send</span>
        <span><span class="kbd">Esc</span> close</span>
        <span><span class="kbd">N</span> next</span>
        <span><span class="kbd">Ctrl+↵</span> allow</span>
        <span style="margin-left:auto;color:var(--ink-4)">the workspace stays put behind you</span>
      </div>
    </div>
  `,
})
export class PeekOverlayComponent implements OnInit, OnDestroy {
  readonly registry = inject(CommandRegistryService);
  private readonly notifications = inject(NotificationService);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly agentsStore = inject(AgentsStore);
  private readonly projects = inject(ProjectActionsService);
  private readonly ui = inject(UiStore);
  readonly mix = mix;

  /** The queue: pending notifications, oldest first — work them in order. */
  readonly items = computed<AgentNotification[]>(() =>
    [...this.notifications.pending()].sort((a, b) => a.createdAt - b.createdAt),
  );
  readonly index = computed(() => {
    const n = this.items().length;
    return n ? Math.min(this.idx(), n - 1) : 0;
  });
  private readonly idx = signal(0);
  readonly current = computed<AgentNotification | null>(() => this.items()[this.index()] ?? null);
  readonly reply = signal("");

  readonly context = computed<{ text: string; kind: ActivityKind }[]>(() => {
    const cur = this.current();
    return cur ? this.runtime.activityFor(cur.agentId).slice(-5) : [];
  });

  agentOf(n: AgentNotification) {
    return this.runtime.agents().find((a) => a.id === n.agentId) ?? null;
  }
  projectOf(n: AgentNotification) {
    const ag = this.agentOf(n);
    return ag ? this.projects.projectOf(ag.projectId) : undefined;
  }

  ngOnInit(): void {
    // opened at a specific notification (bell / status-bar click) → jump to it
    const initial = this.registry.overlay()?.tab;
    if (initial) {
      const i = this.items().findIndex((n) => n.id === initial);
      if (i >= 0) this.idx.set(i);
    }
    window.addEventListener("keydown", this.onKey, true);
  }
  ngOnDestroy(): void {
    window.removeEventListener("keydown", this.onKey, true);
  }

  /** Queue keys — inert while the user is typing (the reply input included). */
  private readonly onKey = (e: KeyboardEvent): void => {
    const target = e.target as HTMLElement | null;
    const tag = target?.tagName ?? "";
    const typing = tag === "INPUT" || tag === "TEXTAREA" || !!target?.isContentEditable;
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      this.allow();
      return;
    }
    if (typing) return;
    if (e.key === "n" || e.key === "N") {
      e.preventDefault();
      e.stopPropagation();
      this.step(e.shiftKey ? -1 : 1);
    }
  };

  step(dir: 1 | -1): void {
    const n = this.items().length;
    if (n) this.idx.set((this.index() + dir + n) % n);
  }

  /** Ctrl+Enter: resolve the current item on its accepting path. */
  allow(): void {
    const cur = this.current();
    if (!cur) return;
    if (cur.kind === "permission") this.notifications.accept(cur);
    else if (cur.kind === "done") this.notifications.review(cur);
    // questions have no single accepting keystroke — pick an option in the card
  }

  send(): void {
    const cur = this.current();
    const text = this.reply().trim();
    if (!cur || !text) return;
    void this.agentsStore.input(cur.agentId, text + "\r").catch(() => {});
    this.ui.flash("sent to " + cur.agentName);
    this.reply.set("");
  }

  prefix(kind: ActivityKind): string {
    switch (kind) {
      case "user":
        return ">";
      case "tool":
        return "$";
      case "error":
        return "!";
      case "question":
        return "?";
      case "success":
        return "✓";
      default:
        return "·";
    }
  }

  ago(ts: number): string {
    const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
    if (s < 60) return s + "s ago";
    const m = Math.round(s / 60);
    if (m < 60) return m + "m ago";
    return Math.round(m / 60) + "h ago";
  }
}
