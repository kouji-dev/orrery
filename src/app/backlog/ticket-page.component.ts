import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  linkedSignal,
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
import { TagBarComponent } from "./tag-bar.component";
import { allTagsOf } from "./tags.util";
import { KjBadgeComponent, KjButtonComponent } from "@kouji-ui/components";
import { SelectComponent } from "../shared/select.component";

// ── Status token map (mirrors design TICKET_COL) ─────────────────────────────
export interface TicketStatusMeta {
  label: string;
  color: string;
}
export const TICKET_STATUS: Record<TicketStatus, TicketStatusMeta> = {
  todo: { label: "To do", color: "var(--ink-3)" },
  inprogress: { label: "In progress", color: "var(--st-running)" },
  done: { label: "Done", color: "var(--st-done)" },
};
export const TICKET_STATUS_ORDER: TicketStatus[] = ["todo", "inprogress", "done"];

// ── Avatar helpers ────────────────────────────────────────────────────────────
const AV_COLORS = [
  "var(--id-1)",
  "var(--id-2)",
  "var(--id-3)",
  "var(--id-4)",
  "var(--id-5)",
  "var(--id-6)",
  "var(--id-7)",
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
  imports: [RichViewComponent, ToolBadgeComponent, KjBadgeComponent],
  template: `
    @let c = comment();
    @let isAgent = c.role === 'agent';
    <div style="display:flex;gap:var(--sp-5)">
      <!-- avatar / tool badge -->
      @if (isAgent && c.tool) {
        <app-tool-badge [tool]="$any(c.tool)" [size]="28" style="flex:none" />
      } @else {
        <span
          [style.width.px]="28"
          [style.height.px]="28"
          [style.color]="avatarColor()"
          [style.background]="avatarBg()"
          [style.border]="avatarBorder()"
          style="flex:none;border-radius:50%;display:grid;place-items:center;font-family:var(--font-disp);font-weight:var(--fw-medium)"
        >{{ avatarInitials() }}</span>
      }
      <div style="flex:1;min-width:0">
        <div style="display:flex;align-items:center;gap:var(--sp-4);margin-bottom:var(--sp-2)">
          <span class="disp" style="font-weight:var(--fw-medium);color:var(--ink)">{{ c.author }}</span>
          @if (isAgent) {
            <kj-badge class="up" fg="var(--ui-ink)" style="--kj-badge---kj-badge-border-color:var(--ui-line)">AGENT</kj-badge>
          }
          <span class="tnum" style="color:var(--ink-4)">{{ when() }}</span>
        </div>
        <div
          [style.background]="isAgent ? 'var(--ui-sel)' : 'var(--panel)'"
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
  imports: [IconComponent, RichEditorComponent, RichViewComponent, ToolBadgeComponent, StatusDotComponent, TicketCommentComponent, TagBarComponent, KjButtonComponent, KjBadgeComponent, SelectComponent],
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
        <div style="display:flex;align-items:center;gap:var(--sp-4);font-size:var(--fs-meta);color:var(--ink-3);margin-bottom:var(--sp-4)">
          <app-icon name="layers" size="sm" [color]="'var(--ui-ink)'" />
          <span>Backlog</span>
          <app-icon name="chevron" size="sm" style="width:var(--sp-5);height:var(--sp-5)" [color]="'var(--ink-4)'" />
          <span class="tnum">{{ isDraft ? 'New ticket' : '#' + (tk ? shortId(tk.id) : '') }}</span>
          <span
            class="up"
            [style.color]="sm.color"
            [style.border]="'1px solid color-mix(in oklch,' + sm.color + ',transparent 62%)'"
            [style.background]="'color-mix(in oklch,' + sm.color + ',transparent 88%)'"
            class="chip up kj-push"
            style="gap:var(--sp-3);padding:var(--sp-1) var(--sp-4);"
          >
            <span class="dot" [style.background]="sm.color" style="animation:none"></span>
            {{ sm.label }}
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
            style="width:100%;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-disp);font-size:var(--fs-xl);font-weight:var(--fw-medium);letter-spacing:-0.02em;padding:0"
          />
        } @else if (tk) {
          <h1 style="font-size:var(--fs-xl);line-height:1.25;text-wrap:balance">
            {{ tk.title }}
          </h1>
        }

        <!-- actions row -->
        <div style="display:flex;align-items:center;gap:var(--sp-4);margin-top:var(--sp-6)">
          @if (isEditing) {
            <kj-button kjVariant="default" [kjDisabled]="!draftTitle().trim()" (click)="save()">
              <app-icon [name]="isDraft ? 'plus' : 'check'" size="sm" />
              {{ isDraft ? 'Create ticket' : 'Save' }}
            </kj-button>
            <kj-button kjVariant="outline" (click)="cancel()">Cancel</kj-button>
          } @else if (tk) {
            @if (tk.status === 'todo') {
              <kj-button kjVariant="default" (click)="dispatch()">
                <app-icon name="bolt" size="sm" />Dispatch agent
              </kj-button>
            }
            @if (ag) {
              <kj-button kjVariant="outline" (click)="openAgent()">
                <app-icon name="enter" size="sm" />Open agent
              </kj-button>
            }
            <kj-button kjVariant="outline" (click)="editing.set(true)">
              <app-icon name="rename" size="sm" />Edit
            </kj-button>
            <kj-button kjVariant="outline" (click)="deleteTicket()" [style.--kj-button-fg]="deleteHovered() ? 'var(--st-blocked)' : 'var(--ink-3)'" (mouseenter)="deleteHovered.set(true)" (mouseleave)="deleteHovered.set(false)">
              <app-icon name="trash" size="sm" />Delete
            </kj-button>
          }
        </div>
      </div>

      <!-- ── Body ── -->
      <div class="scroll-y" style="flex:1">
        <div style="display:grid;grid-template-columns:minmax(0,1fr) 268px;gap:var(--sp-10);padding:var(--sp-8) var(--sp-9);max-width: round(calc(1040px * var(--density)), 1px);margin:0 auto;align-items:start">

          <!-- ── MAIN COLUMN ── -->
          <div style="display:flex;flex-direction:column;gap:var(--sp-9);min-width:0">

            <!-- Notes -->
            <div>
              <div class="up" style="color:var(--ink-3);margin-bottom:var(--sp-4)">What should be done</div>
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
                  <div style="color:var(--ink-4);font-style:italic">No description yet.</div>
                }
              }
            </div>

            <!-- Comments (not in draft mode) -->
            @if (!isDraft) {
              <div style="display:flex;flex-direction:column;gap:var(--sp-6)">
                <!-- section header -->
                <div class="up" style="color:var(--ink-3);display:flex;align-items:center;gap:var(--sp-3)">
                  <app-icon name="chat" size="sm" />
                  Comments
                  <kj-badge class="tnum">{{ comments().length }}</kj-badge>
                </div>

                @if (comments().length === 0) {
                  <div style="font-size:var(--fs-meta);color:var(--ink-4)">No comments yet — start the thread.</div>
                } @else {
                  @for (c of comments(); track c.id) {
                    <app-ticket-comment [comment]="c" />
                  }
                }

                <!-- composer -->
                <div style="display:flex;gap:var(--sp-5);margin-top:var(--sp-1)">
                  <!-- "You" avatar -->
                  <span
                    style="flex:none;width:var(--ctl-h);height:var(--ctl-h);border-radius:50%;display:grid;place-items:center;font-family:var(--font-disp);font-weight:var(--fw-medium)"
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
                      <kj-button kjVariant="default" [kjDisabled]="!composerHasContent()" (click)="postComment()">
                        <app-icon name="chat" size="sm" />Comment
                      </kj-button>
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
              <div role="group" aria-label="Ticket status" style="display:flex;gap:var(--sp-2)">
                @for (s of statusOrder; track s) {
                  @let sm2 = ticketStatusMeta(s);
                  @let on = (isDraft ? 'todo' : (tk?.status ?? 'todo')) === s;
                  <kj-button kjVariant="ghost" [kjFullWidth]="true" (click)="moveStatus(s)" class="kj-center" [style.--kj-button-bg]="on ? 'color-mix(in oklch,' + sm2.color + ',transparent 86%)' : 'transparent'" [style.--kj-button-fg]="on ? sm2.color : 'var(--ink-3)'" [style.box-shadow]="on ? '0 0 0 1px color-mix(in oklch,' + sm2.color + ',transparent 60%)' : 'none'">
                    <span class="dot" [style.background]="sm2.color" style="animation:none;width:var(--sp-3);height:var(--sp-3)"></span>
                    {{ sm2.label }}
                  </kj-button>
                }
              </div>
            </div>

            <!-- Project -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Project</span>
              @if (isEditing) {
                <app-select [value]="draftProjectId()" [options]="projectOptions()" (valueChange)="draftProjectId.set($event)" />
              } @else {
                @let proj = resolvedProject();
                @if (proj) {
                  <span style="display:inline-flex;align-items:center;gap:var(--sp-3)" [style.color]="proj.color">
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
                  <span style="color:var(--ink-4)">No project</span>
                }
              }
            </div>

            <!-- Tags (live add/remove via search-or-create picker) -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Tags</span>
              <app-tag-bar
                [allTags]="allTags()"
                [tags]="currentTags()"
                (tagsChange)="onTagsChange($event)"
              />
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
                    <h4 class="trunc">{{ ag.name }}</h4>
                    <div style="display:flex;align-items:center;gap:var(--sp-2);margin-top:var(--sp-1)" [style.color]="agentMeta().color">
                      <app-status-dot [status]="ag.status" />{{ agentMeta().label }}
                    </div>
                  </div>
                  <app-icon name="enter" size="sm" [color]="'var(--ink-4)'" style="flex:none" />
                </div>
              } @else if (tk?.status === 'todo') {
                <kj-button kjVariant="outline" class="kj-center" style="--kj-button-fg: var(--ui-ink); --kj-button-border-color: var(--ui-line)" (click)="dispatch()">
                  <app-icon name="bolt" size="sm" />Dispatch an agent
                </kj-button>
              } @else {
                <span style="color:var(--ink-4)">None attached</span>
              }
            </div>

            <!-- Branch (only when agent exists) -->
            @if (ag) {
              <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
                <span class="field-label" style="margin:0">Branch</span>
                <span style="display:inline-flex;align-items:center;gap:var(--sp-3);color:var(--ink-2)">
                  <app-icon name="branch" size="sm" style="width:var(--sp-6);height:var(--sp-6)" [color]="'var(--ink-3)'" />
                  {{ ag.branch.replace('agent/', '') }}
                </span>
              </div>
            }

            <!-- Created -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              <span class="field-label" style="margin:0">Created</span>
              <span class="tnum" style="color:var(--ink-2)">
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
  // Each draft field is a linkedSignal seeded from the current ticket: it
  // re-seeds whenever the ticket id (or the stored ticket) changes, and stays
  // locally writable while editing — replacing the old reset-everything effect.
  readonly editing = linkedSignal({ source: this.ticketId, computation: (id) => id === "draft" });
  readonly draftTitle = linkedSignal({
    source: this.ticketId,
    computation: (id) => (id === "draft" ? "" : (this.ticketsStore.byId(id)?.title ?? "")),
  });
  readonly draftNotes = linkedSignal({
    source: this.ticketId,
    computation: (id) => (id === "draft" ? "" : (this.ticketsStore.byId(id)?.notes ?? "")),
  });
  readonly draftProjectId = linkedSignal({
    source: this.ticketId,
    computation: (id) => (id === "draft" ? "" : (this.ticketsStore.byId(id)?.projectId ?? "")),
  });
  readonly draftTags = linkedSignal<string, string[]>({
    source: this.ticketId,
    computation: (id) => (id === "draft" ? [] : (this.ticketsStore.byId(id)?.tags ?? [])),
  });
  readonly deleteHovered = linkedSignal({ source: this.ticketId, computation: () => false });
  readonly agentHovered = signal(false);

  // ── comments state ────────────────────────────────────────────────────────
  readonly comments = signal<Comment[]>([]);
  readonly composerBody = linkedSignal({ source: this.ticketId, computation: () => "" });
  readonly composerResetSignal = signal(0);

  /** Options for the project select (edit mode). */
  readonly projectOptions = computed(() => [
    { value: "", label: "No project" },
    ...this.projects.all().map((p) => ({ value: p.id, label: p.name })),
  ]);

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

  // ── tags ──────────────────────────────────────────────────────────────────
  /** The tag universe (for search/suggest), derived from every ticket. */
  readonly allTags = computed(() => allTagsOf(this.ticketsStore.all()));
  /** Tags shown/edited: a draft stages locally; a real ticket reads its own. */
  readonly currentTags = computed(() =>
    this.ticketId() === "draft" ? this.draftTags() : (this.ticket()?.tags ?? []),
  );

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
    // The only real side effect left on a ticket switch: (re)load its comments.
    // Draft-field resets live on the linkedSignals above.
    effect(() => {
      const id = this.ticketId();
      if (id !== "draft") void this.loadComments(id);
      else this.comments.set([]);
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
          tags: this.draftTags(),
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

  /** Tags edited from the side rail. A draft stages locally (saved on create);
   *  a real ticket persists immediately and refreshes from the ticket:// event. */
  onTagsChange(next: string[]): void {
    const id = this.ticketId();
    if (id === "draft") {
      this.draftTags.set(next);
      return;
    }
    this.draftTags.set(next);
    void this.ticketsStore.update(id, { tags: next });
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
