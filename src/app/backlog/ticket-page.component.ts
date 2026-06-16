import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { Comment, TicketStatus } from "../models";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { RichEditorComponent } from "../shared/rich-editor/rich-editor.component";
import { RichViewComponent } from "../shared/rich-editor/rich-view.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { STATUS_META } from "../utils";

// ── Status token map (mirrors design TICKET_COL) ─────────────────────────────
export interface TicketStatusMeta {
  label: string;
  color: string;
}
export const TICKET_STATUS: Record<TicketStatus, TicketStatusMeta> = {
  todo: { label: "To do", color: "var(--ink-3)" },
  inprogress: { label: "In progress", color: "var(--accent)" },
  done: { label: "Done", color: "var(--st-done)" },
};
export const TICKET_STATUS_ORDER: TicketStatus[] = ["todo", "inprogress", "done"];

// ── Avatar helpers ────────────────────────────────────────────────────────────
const AV_COLORS = [
  "#a855f7",
  "#22d3ee",
  "#34e0a1",
  "#ff6b35",
  "#ff4d8d",
  "#3b82f6",
  "#f5c451",
];
export function avColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = ((h * 31 + name.charCodeAt(i)) >>> 0);
  return AV_COLORS[h % AV_COLORS.length];
}
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/);
  return (
    ((parts[0] || "")[0] || "?").toUpperCase() +
    (parts.length > 1 ? (parts[parts.length - 1][0] || "").toUpperCase() : "")
  );
}

// ── Helpers ───────────────────────────────────────────────────────────────────
export function plainTextNonEmpty(html: string | null | undefined): boolean {
  if (!html) return false;
  return html.replace(/<[^>]*>/g, "").trim().length > 0;
}

export function shortId(id: string): string {
  return id.replace(/^t/, "");
}

export function relativeTime(ts: number): string {
  const diff = Math.floor((Date.now() - ts) / 1000);
  if (diff < 60) return "just now";
  if (diff < 3600) return Math.floor(diff / 60) + "m ago";
  if (diff < 86400) return Math.floor(diff / 3600) + "h ago";
  return Math.floor(diff / 86400) + "d ago";
}

export function fmtCreated(ts: number): string {
  return new Date(ts).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

// ── Comment row component ─────────────────────────────────────────────────────
@Component({
  selector: "app-ticket-comment",
  changeDetection: ChangeDetectionStrategy.OnPush,
  standalone: true,
  imports: [RichViewComponent, ToolBadgeComponent],
  template: `
    @let c = comment();
    @let isAgent = c.role === 'agent';
    <div style="display:flex;gap:var(--sp-5)">
      <!-- avatar / tool badge -->
      @if (isAgent && c.tool) {
        <div style="flex:none;width:var(--ctl-h);height:var(--ctl-h);display:grid;place-items:center">
          <app-tool-badge [tool]="$any(c.tool)" [size]="28" />
        </div>
      } @else {
        <span
          [style.width.px]="28"
          [style.height.px]="28"
          [style.color]="avatarColor()"
          [style.background]="avatarBg()"
          [style.border]="avatarBorder()"
          style="flex:none;border-radius:50%;display:grid;place-items:center;font-family:var(--font-disp);font-size:var(--fs-xs);font-weight:600"
        >{{ avatarInitials() }}</span>
      }
      <div style="flex:1;min-width:0">
        <div style="display:flex;align-items:center;gap:var(--sp-4);margin-bottom:var(--sp-2)">
          <span class="disp" style="font-size:var(--fs-ui);font-weight:600;color:var(--ink)">{{ c.author }}</span>
          @if (isAgent) {
            <span class="chip up" style="font-size:var(--fs-3xs);padding:1px var(--sp-3);color:var(--accent-2);border-color:color-mix(in oklch,var(--accent-2),transparent 60%)">AGENT</span>
          }
          <span class="tnum" style="font-size:var(--fs-xs);color:var(--ink-4)">{{ when() }}</span>
        </div>
        <div
          [style.background]="isAgent ? 'color-mix(in oklch,var(--accent-2),transparent 94%)' : 'var(--panel)'"
          style="border:1px solid var(--hair);border-radius:var(--r-md);padding:var(--sp-4) var(--sp-6)"
        >
          <app-rich-view [html]="c.body" [compact]="true" />
        </div>
      </div>
    </div>
  `,
})
export class TicketCommentComponent {
  readonly comment = input.required<Comment>();

  readonly avatarColor = computed(() => avColor(this.comment().author));
  readonly avatarBg = computed(() => {
    const c = avColor(this.comment().author);
    return `color-mix(in oklch,${c},transparent 84%)`;
  });
  readonly avatarBorder = computed(() => {
    const c = avColor(this.comment().author);
    return `1px solid color-mix(in oklch,${c},transparent 62%)`;
  });
  readonly avatarInitials = computed(() => initials(this.comment().author));
  readonly when = computed(() => relativeTime(this.comment().createdAt));
}

// ── Ticket Page ───────────────────────────────────────────────────────────────
@Component({
  selector: "app-ticket-page",
  changeDetection: ChangeDetectionStrategy.OnPush,
  standalone: true,
  imports: [
    IconComponent,
    RichEditorComponent,
    RichViewComponent,
    ToolBadgeComponent,
    StatusDotComponent,
    TicketCommentComponent,
  ],
  template: `
    @let tk = ticket();
    @let isDraft = ticketId() === 'draft';
    @let isEditing = editing();
    @let ag = resolvedAgent();
    @let sm = statusMeta();

    <div style="display:flex;flex-direction:column;min-height:0;height:100%;background:var(--panel-2)">

      <!-- ── Header ── -->
      <div style="padding:var(--sp-6) var(--sp-8);border-bottom:1px solid var(--hair);background:var(--panel);flex:none">

        <!-- breadcrumb + status pill -->
        <div style="display:flex;align-items:center;gap:var(--sp-4);font-size:var(--fs-sm);color:var(--ink-3);margin-bottom:var(--sp-4)">
          <app-icon name="layers" size="sm" [color]="'var(--accent)'" />
          <span>Backlog</span>
          <app-icon name="chevron" size="sm" style="width:var(--sp-5);height:var(--sp-5)" [color]="'var(--ink-4)'" />
          <span class="tnum">{{ isDraft ? 'New ticket' : '#' + (tk ? shortId(tk.id) : '') }}</span>
          <span style="margin-left:auto">
            <span
              class="up"
              [style.color]="sm.color"
              [style.border]="'1px solid color-mix(in oklch,' + sm.color + ',transparent 62%)'"
              [style.background]="'color-mix(in oklch,' + sm.color + ',transparent 88%)'"
              style="display:inline-flex;align-items:center;gap:var(--sp-3);font-size:var(--fs-2xs);padding:var(--sp-1) var(--sp-4);border-radius:999px;letter-spacing:0.1em"
            >
              <span class="dot" [style.background]="sm.color" style="animation:none"></span>
              {{ sm.label }}
            </span>
          </span>
        </div>

        <!-- title -->
        @if (isEditing) {
          <input
            #titleInput
            [value]="draftTitle()"
            (input)="draftTitle.set($any($event.target).value)"
            (keydown.enter)="save()"
            placeholder="Ticket title — what needs to be done?"
            style="width:100%;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-disp);font-size:var(--fs-xl);font-weight:600;letter-spacing:-0.02em;padding:0"
          />
        } @else if (tk) {
          <h1 class="disp" style="font-size:var(--fs-xl);font-weight:600;letter-spacing:-0.02em;line-height:1.25;text-wrap:balance">
            {{ tk.title }}
          </h1>
        }

        <!-- actions row -->
        <div style="display:flex;align-items:center;gap:var(--sp-4);margin-top:var(--sp-6)">
          @if (isEditing) {
            <button class="btn primary" [disabled]="!draftTitle().trim()" (click)="save()">
              <app-icon [name]="isDraft ? 'plus' : 'check'" size="sm" />
              {{ isDraft ? 'Create ticket' : 'Save' }}
            </button>
            <button class="btn ghost-hair" (click)="cancel()">Cancel</button>
          } @else if (tk) {
            @if (tk.status === 'todo') {
              <button class="btn primary" (click)="dispatch()">
                <app-icon name="bolt" size="sm" />Dispatch agent
              </button>
            }
            @if (ag) {
              <button class="btn ghost-hair" (click)="openAgent()">
                <app-icon name="enter" size="sm" />Open agent
              </button>
            }
            <button class="btn ghost-hair" (click)="editing.set(true)">
              <app-icon name="rename" size="sm" />Edit
            </button>
            <button
              class="btn ghost-hair"
              (click)="deleteTicket()"
              [style.color]="deleteHovered() ? 'var(--st-blocked)' : 'var(--ink-3)'"
              (mouseenter)="deleteHovered.set(true)"
              (mouseleave)="deleteHovered.set(false)"
            >
              <app-icon name="trash" size="sm" />Delete
            </button>
          }
        </div>
      </div>

      <!-- ── Body ── -->
      <div class="scroll-y" style="flex:1">
        <div style="display:grid;grid-template-columns:minmax(0,1fr) 268px;gap:var(--sp-10);padding:var(--sp-8) var(--sp-9);max-width:1040px;margin:0 auto;align-items:start">

          <!-- ── MAIN COLUMN ── -->
          <div style="display:flex;flex-direction:column;gap:var(--sp-9);min-width:0">

            <!-- Notes -->
            <div>
              <div class="up" style="font-size:var(--fs-xs);color:var(--ink-3);margin-bottom:var(--sp-4)">What should be done</div>
              @if (isEditing) {
                <app-rich-editor
                  [value]="draftNotes()"
                  (valueChange)="draftNotes.set($event)"
                  [minHeight]="150"
                  placeholder="Describe the work — context, acceptance criteria, links…"
                />
              } @else if (tk) {
                @if (plainTextNonEmpty(tk.notes)) {
                  <app-rich-view [html]="tk.notes" />
                } @else {
                  <div style="font-size:var(--fs-ui);color:var(--ink-4);font-style:italic">No description yet.</div>
                }
              }
            </div>

            <!-- Comments (not in draft mode) -->
            @if (!isDraft) {
              <div style="display:flex;flex-direction:column;gap:var(--sp-6)">
                <!-- section header -->
                <div class="up" style="font-size:var(--fs-xs);color:var(--ink-3);display:flex;align-items:center;gap:var(--sp-3)">
                  <app-icon name="chat" size="sm" />
                  Comments
                  <span class="chip tnum" style="font-size:var(--fs-2xs);padding:0 var(--sp-3)">{{ comments().length }}</span>
                </div>

                @if (comments().length === 0) {
                  <div style="font-size:var(--fs-sm);color:var(--ink-4)">No comments yet — start the thread.</div>
                } @else {
                  @for (c of comments(); track c.id) {
                    <app-ticket-comment [comment]="c" />
                  }
                }

                <!-- composer -->
                <div style="display:flex;gap:var(--sp-5);margin-top:var(--sp-1)">
                  <!-- "You" avatar -->
                  <span
                    style="flex:none;width:var(--ctl-h);height:var(--ctl-h);border-radius:50%;display:grid;place-items:center;font-family:var(--font-disp);font-size:var(--fs-xs);font-weight:600"
                    [style.color]="youAvatarColor"
                    [style.background]="youAvatarBg"
                    [style.border]="youAvatarBorder"
                  >Y</span>
                  <div style="flex:1;min-width:0;display:flex;flex-direction:column;gap:var(--sp-4)">
                    <app-rich-editor
                      [value]="''"
                      (valueChange)="composerBody.set($event)"
                      [minHeight]="70"
                      [compact]="true"
                      [resetSignal]="composerResetSignal()"
                      placeholder="Leave a comment…  ⌘B bold · ⌘K link"
                    />
                    <div style="display:flex;justify-content:flex-end">
                      <button
                        class="btn primary"
                        [disabled]="!composerHasContent()"
                        (click)="postComment()"
                      >
                        <app-icon name="chat" size="sm" />Comment
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            }
          </div>

          <!-- ── SIDE RAIL ── -->
          <div style="display:flex;flex-direction:column;gap:var(--sp-7)">

            <!-- Status segmented control -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Status</span>
              <div style="display:flex;gap:var(--sp-2);padding:var(--sp-1);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md)">
                @for (s of statusOrder; track s) {
                  @let sm2 = ticketStatusMeta(s);
                  @let on = (isDraft ? 'todo' : (tk?.status ?? 'todo')) === s;
                  <button
                    class="btn"
                    (click)="moveStatus(s)"
                    [style.flex]="1"
                    style="justify-content:center;padding:var(--sp-2) var(--sp-2);font-size:var(--fs-xs);border-radius:var(--r-sm);gap:var(--sp-2)"
                    [style.background]="on ? 'color-mix(in oklch,' + sm2.color + ',transparent 86%)' : 'transparent'"
                    [style.color]="on ? sm2.color : 'var(--ink-3)'"
                    [style.box-shadow]="on ? '0 0 0 1px color-mix(in oklch,' + sm2.color + ',transparent 60%)' : 'none'"
                  >
                    <span class="dot" [style.background]="sm2.color" style="animation:none;width:var(--sp-3);height:var(--sp-3)"></span>
                    {{ sm2.label }}
                  </button>
                }
              </div>
            </div>

            <!-- Project -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Project</span>
              @if (isEditing) {
                <select class="osel" [value]="draftProjectId()" (change)="draftProjectId.set($any($event.target).value)">
                  <option value="">No project</option>
                  @for (p of projects.all(); track p.id) {
                    <option [value]="p.id">{{ p.name }}</option>
                  }
                </select>
              } @else {
                @let proj = resolvedProject();
                @if (proj) {
                  <span style="display:inline-flex;align-items:center;gap:var(--sp-3);font-size:var(--fs-ui)" [style.color]="proj.color">
                    <span
                      style="width:19px;height:19px;border-radius:5px;display:grid;place-items:center;flex:none"
                      [style.background]="'color-mix(in oklch,' + proj.color + ',transparent 82%)'"
                      [style.border]="'1px solid color-mix(in oklch,' + proj.color + ',transparent 62%)'"
                    >
                      <app-icon [name]="proj.icon" size="sm" [color]="proj.color" style="width:var(--sp-6);height:var(--sp-6)" />
                    </span>
                    {{ proj.name }}
                  </span>
                } @else {
                  <span style="font-size:var(--fs-ui);color:var(--ink-4)">No project</span>
                }
              }
            </div>

            <!-- Agent -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Agent</span>
              @if (ag && tk) {
                <div
                  (click)="openAgent()"
                  (mouseenter)="agentHovered.set(true)"
                  (mouseleave)="agentHovered.set(false)"
                  style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-5);border-radius:var(--r-md);background:var(--panel);cursor:pointer;transition:border-color .15s"
                  [style.border]="agentHovered() ? '1px solid var(--hair-2)' : '1px solid var(--hair)'"
                >
                  <app-tool-badge [tool]="ag.tool" [size]="20" />
                  <div style="min-width:0;flex:1">
                    <div class="disp" style="font-size:var(--fs-ui);font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.name }}</div>
                    <div style="display:flex;align-items:center;gap:var(--sp-2);font-size:var(--fs-xs);margin-top:var(--sp-1)" [style.color]="agentMeta().color">
                      <app-status-dot [status]="ag.status" />{{ agentMeta().label }}
                    </div>
                  </div>
                  <app-icon name="enter" size="sm" [color]="'var(--ink-4)'" style="flex:none" />
                </div>
              } @else if (tk?.status === 'todo') {
                <button
                  class="btn ghost-hair"
                  (click)="dispatch()"
                  style="justify-content:center;color:var(--accent);border-color:color-mix(in oklch,var(--accent),transparent 55%)"
                >
                  <app-icon name="bolt" size="sm" />Dispatch an agent
                </button>
              } @else {
                <span style="font-size:var(--fs-ui);color:var(--ink-4)">None attached</span>
              }
            </div>

            <!-- Branch (only when agent exists) -->
            @if (ag) {
              <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
                <span class="field-label" style="margin:0">Branch</span>
                <span style="display:inline-flex;align-items:center;gap:var(--sp-3);font-size:var(--fs-sm);color:var(--ink-2)">
                  <app-icon name="branch" size="sm" style="width:var(--sp-6);height:var(--sp-6)" [color]="'var(--accent-2)'" />
                  {{ ag.branch.replace('agent/', '') }}
                </span>
              </div>
            }

            <!-- Created -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Created</span>
              <span class="tnum" style="font-size:var(--fs-sm);color:var(--ink-2)">
                {{ isDraft ? 'just now' : (tk ? fmtCreated(tk.createdAt) : '—') }}
              </span>
            </div>

          </div>
        </div>
      </div>
    </div>
  `,
})
export class TicketPageComponent {
  readonly ticketId = input.required<string>();

  readonly ticketsStore = inject(TicketsStore);
  readonly ui = inject(UiStore);
  private readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);

  // ── editing state ─────────────────────────────────────────────────────────
  readonly editing = signal(false);
  readonly draftTitle = signal("");
  readonly draftNotes = signal("");
  readonly draftProjectId = signal("");
  readonly deleteHovered = signal(false);
  readonly agentHovered = signal(false);

  // ── comments state ────────────────────────────────────────────────────────
  readonly comments = signal<Comment[]>([]);
  readonly composerBody = signal("");
  readonly composerResetSignal = signal(0);

  readonly composerHasContent = computed(() =>
    this.composerBody().replace(/<[^>]*>/g, "").trim().length > 0,
  );

  // ── derived ticket ────────────────────────────────────────────────────────
  readonly ticket = computed(() => {
    const id = this.ticketId();
    if (id === "draft") return null;
    return this.ticketsStore.byId(id) ?? null;
  });

  readonly resolvedAgent = computed(() => {
    const tk = this.ticket();
    if (!tk?.agentId) return null;
    return this.runtime.agents().find((a) => a.id === tk.agentId) ?? null;
  });

  readonly resolvedProject = computed(() => {
    // in edit mode use the draft project id
    const id = this.editing() ? this.draftProjectId() : (this.ticket()?.projectId ?? "");
    return id ? this.projects.projectOf(id) : undefined;
  });

  readonly statusMeta = computed((): TicketStatusMeta => {
    const id = this.ticketId();
    if (id === "draft") return TICKET_STATUS["todo"];
    const tk = this.ticket();
    return tk ? TICKET_STATUS[tk.status] : TICKET_STATUS["todo"];
  });

  readonly agentMeta = computed(() => {
    const ag = this.resolvedAgent();
    if (!ag) return STATUS_META["idle"];
    return STATUS_META[ag.status] ?? STATUS_META["idle"];
  });

  // ── static helpers (template-accessible) ─────────────────────────────────
  readonly statusOrder = TICKET_STATUS_ORDER;
  readonly plainTextNonEmpty = plainTextNonEmpty;
  readonly shortId = shortId;
  readonly fmtCreated = fmtCreated;

  ticketStatusMeta(s: TicketStatus): TicketStatusMeta {
    return TICKET_STATUS[s];
  }

  // "You" avatar vars (constant, so compute once)
  readonly youAvatarColor = avColor("You");
  readonly youAvatarBg = `color-mix(in oklch,${avColor("You")},transparent 84%)`;
  readonly youAvatarBorder = `1px solid color-mix(in oklch,${avColor("You")},transparent 62%)`;

  constructor() {
    // When ticketId changes: reset edit state + reload comments
    effect(() => {
      const id = this.ticketId();
      const isDraft = id === "draft";
      const tk = isDraft ? null : this.ticketsStore.byId(id);

      this.editing.set(isDraft);
      this.draftTitle.set(tk?.title ?? "");
      this.draftNotes.set(tk?.notes ?? "");
      this.draftProjectId.set(tk?.projectId ?? "");
      this.deleteHovered.set(false);
      this.composerBody.set("");

      if (!isDraft) {
        void this.loadComments(id);
      } else {
        this.comments.set([]);
      }
    });
  }

  private async loadComments(ticketId: string): Promise<void> {
    try {
      const list = await this.ticketsStore.comments(ticketId);
      this.comments.set(list);
    } catch {
      this.comments.set([]);
    }
  }

  // ── actions ───────────────────────────────────────────────────────────────

  async save(): Promise<void> {
    const title = this.draftTitle().trim();
    if (!title) return;
    const isDraft = this.ticketId() === "draft";

    if (isDraft) {
      try {
        const created = await this.ticketsStore.create({
          title,
          notes: this.draftNotes() || undefined,
          projectId: this.draftProjectId() || null,
        });
        // Close the draft tab and open the new ticket
        const draftTab = this.ui.tabs().find(
          (t) => t.kind === "ticket" && t.ticketId === "draft",
        );
        if (draftTab) this.ui.closeTab(draftTab.id);
        this.ui.openTicket(created.id);
      } catch {
        /* backend unavailable */
      }
    } else {
      const id = this.ticketId();
      try {
        await this.ticketsStore.update(id, {
          title,
          notes: this.draftNotes() || undefined,
          projectId: this.draftProjectId() || null,
        });
      } catch {
        /* ignore */
      }
      this.editing.set(false);
    }
  }

  cancel(): void {
    const isDraft = this.ticketId() === "draft";
    if (isDraft) {
      const draftTab = this.ui.tabs().find(
        (t) => t.kind === "ticket" && t.ticketId === "draft",
      );
      if (draftTab) this.ui.closeTab(draftTab.id);
      return;
    }
    const tk = this.ticket();
    this.draftTitle.set(tk?.title ?? "");
    this.draftNotes.set(tk?.notes ?? "");
    this.draftProjectId.set(tk?.projectId ?? "");
    this.editing.set(false);
  }

  async deleteTicket(): Promise<void> {
    const id = this.ticketId();
    if (id === "draft") return;
    const tab = this.ui.tabs().find((t) => t.kind === "ticket" && t.ticketId === id);
    try {
      await this.ticketsStore.remove(id);
    } catch {
      /* ignore */
    }
    if (tab) this.ui.closeTab(tab.id);
  }

  dispatch(): void {
    const tk = this.ticket();
    if (!tk) return;
    this.ui.dispatchTicket({ id: tk.id, projectId: tk.projectId });
  }

  openAgent(): void {
    const ag = this.resolvedAgent();
    if (ag) this.ui.openAgent(ag.id);
  }

  moveStatus(s: TicketStatus): void {
    const id = this.ticketId();
    if (id === "draft") return;
    void this.ticketsStore.setStatus(id, s);
  }

  async postComment(): Promise<void> {
    const body = this.composerBody();
    if (!body.replace(/<[^>]*>/g, "").trim()) return;
    const id = this.ticketId();
    if (id === "draft") return;
    try {
      await this.ticketsStore.addComment(id, { body });
      await this.loadComments(id);
      this.composerResetSignal.update((n) => n + 1);
      this.composerBody.set("");
    } catch {
      /* ignore */
    }
  }
}
