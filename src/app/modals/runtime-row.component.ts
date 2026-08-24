import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  Input,
  signal,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { BRIDGE } from "../data-source/bridge";
import { SettingsStore } from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { KjBadgeComponent, KjButtonComponent, KjInputComponent, KjSpinnerComponent } from "@kouji-ui/components";

/**
 * One CLI runtime row in Settings → Agent defaults. Shows the resolved
 * executable path for a detected tool, or an inline "locate the binary" editor
 * when the tool is missing or was found-but-couldn't-run. Verifying a path runs
 * the backend `<tool> --version` probe; on success the path is persisted to
 * Settings (`toolPath`) and used when spawning that tool.
 */
@Component({
  selector: "app-runtime-row",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ToolBadgeComponent, KjBadgeComponent, KjButtonComponent, KjInputComponent, KjSpinnerComponent],
  template: `
    @let d = det();
    <div class="set-rt" [class.warn]="d?.status === 'error'">
      <div class="set-rt-top">
        <app-tool-badge [tool]="toolId()" [size]="22" />
        <div class="set-rt-id">
          <div class="set-rt-name">{{ toolName() }}</div>
          <div class="set-rt-st" [class.ok]="isOk()" [class.err]="d?.status === 'error'"
            [class.miss]="!d || d?.status === 'missing'">
            <span class="rd"></span>{{ statusLabel() }}
          </div>
        </div>
        @if (isOk() && !editing()) {
          <kj-badge class="set-rt-pathchip">
            <app-icon name="terminal" size="sm" /><span class="pt trunc" [title]="d?.path">{{ d?.path }}</span>
          </kj-badge>
          <kj-button kjVariant="ghost" class="set-rt-link" kjVariant="quiet" (click)="begin()">
            <app-icon name="rename" size="sm" />Change
          </kj-button>
          @if (d?.source === 'manual') {
            <kj-button class="set-rt-link" kjVariant="quiet" (click)="revert()" title="Re-run auto-detection">revert</kj-button>
          }
        }
      </div>

      @if (isOk() && d?.shim && !editing()) {
        <div class="set-rt-shim">
          <app-icon name="flag" size="sm" />
          <span>npm shim install — the native installer launches without a shell wrapper (less RAM per agent).</span>
        </div>
      }

      @if (showEditor()) {
        <div class="set-rt-edit">
          @if (d?.status === 'error' && d?.reason) {
            <div class="set-rt-reason">
              <app-icon name="flag" size="sm" />
              <span>{{ d?.reason }} Last tried <code>{{ d?.path }}</code>.</span>
            </div>
          }
          <div class="set-rt-field">
            <div class="set-rt-input">
              <app-icon name="terminal" size="sm" />
              <kj-input [value]="draft()" [placeholder]="'/path/to/' + toolId()"
                (input)="onDraft($any($event.target).value)"
                (keydown.enter)="verify()" (keydown.escape)="onEscape()" />
            </div>
            <div class="set-rt-act">
              <kj-button kjVariant="outline" (click)="browse()">
                <app-icon name="folderOpen" size="sm" />Browse
              </kj-button>
              <kj-button kjVariant="default" [kjDisabled]="checking()" (click)="verify()">
                @if (checking()) {
                  <kj-spinner kjAriaLabel="Verifying" />
                } @else {
                  <app-icon name="check" size="sm" />
                }
                {{ checking() ? 'Verifying…' : (isOk() ? 'Save' : 'Use this path') }}
              </kj-button>
            </div>
          </div>
          <div style="display:flex;align-items:center;gap:var(--sp-6)">
            @if (fail()) {
              <span class="set-rt-fail"><app-icon name="x" size="sm" />{{ fail() }}</span>
            } @else {
              <span style="color:var(--ink-4)">Orrery runs
                <code>{{ toolId() }} --version</code> to confirm it launches.</span>
            }
            @if (isOk()) { <kj-button class="set-rt-cancel" kjVariant="quiet" (click)="cancel()">Cancel</kj-button> }
          </div>
        </div>
      }
    </div>
  `,
  styles: [
    `
    /* As a flex child of the wide set-row value cell, allow shrinking so a long
       path ellipsizes inside the panel instead of overflowing it. */
    :host{display:block;flex:1 1 auto;min-width:0;max-width:100%;}
    .set-rt{display:flex;flex-direction:column;gap:var(--sp-5);padding:var(--sp-5) var(--sp-6);border-radius:11px;
      border:1px solid var(--hair);background:var(--panel-2);width:100%;max-width:100%;box-sizing:border-box;}
    .set-rt.warn{border-color:color-mix(in oklch,var(--set-amber),transparent 62%);
      background:color-mix(in oklch,var(--set-amber),transparent 93%);}
    .set-rt-top{display:flex;align-items:center;gap:var(--sp-5);min-width:0;}
    .set-rt-id{flex:1;display:flex;flex-direction:column;gap:var(--sp-1);max-width: round(calc(220px * var(--density)), 1px);}
    .set-rt-name{color:var(--ink);font-weight:var(--fw-medium);line-height:1.1;}
    .set-rt-st{font-size:var(--fs-meta);display:flex;align-items:center;gap:var(--sp-2);color:var(--ink-4);white-space:nowrap;}
    .set-rt-st .rd{width:var(--sp-3);height:var(--sp-3);border-radius:50%;flex:none;background:var(--ink-4);}
    .set-rt-st.ok{color:var(--st-done);} .set-rt-st.ok .rd{background:var(--st-done);box-shadow:0 0 7px -1px var(--st-done);}
    .set-rt-st.err{color:var(--set-amber);} .set-rt-st.err .rd{background:var(--set-amber);box-shadow:0 0 7px -1px var(--set-amber);}
    .set-rt-st.miss .rd{background:transparent;border:1px solid var(--hair-2);box-shadow:none;}
    .set-rt-pathchip ::ng-deep .kj-badge{flex:1 1 auto;min-width:0;max-width:100%;display:flex;align-items:center;gap:var(--sp-3);height:var(--row-h);
      padding:0 var(--sp-5);background:var(--panel);border:1px solid var(--hair);border-radius:8px;color:var(--ink-2);
      font:var(--fs-sm) / 1.2 var(--font-ui);}
    .set-rt-pathchip app-icon{color:var(--ink-4);flex:none;display:inline-flex;}
    .set-rt-pathchip .pt{font-family:var(--font-mono);}
    /* .kj-quiet is the bare-label recipe; only the ink ramp, type and the
       tighter icon gap are per-instance here. */
    .set-rt-link ::ng-deep .kj-button{--kj-button-fg:var(--ink-4);--kj-button-font:var(--font-ui);
      --kj-button---kj-button-gap:var(--sp-2);flex:none;}
    .set-rt-link ::ng-deep .kj-button:hover:not([aria-disabled="true"]){--kj-button-fg:var(--ui-ink);}
    .set-rt-shim{display:flex;align-items:flex-start;gap:var(--sp-3);padding-left:var(--sp-11);
      font-size:var(--fs-meta);color:var(--ink-4);line-height:1.45;}
    .set-rt-shim app-icon{flex:none;margin-top:1px;}
    .set-rt-edit{display:flex;flex-direction:column;gap:var(--sp-4);padding-left:var(--sp-11);}
    .set-rt-reason{display:flex;align-items:flex-start;gap:var(--sp-3);color:var(--set-amber);line-height:1.45;}
    .set-rt-reason app-icon{flex:none;margin-top:1px;}
    .set-rt-reason code{background:color-mix(in oklch,var(--set-amber),transparent 86%);
      padding:0 var(--sp-2);border-radius:4px;}
    .set-rt-field{display:flex;align-items:center;gap:var(--sp-4);}
    .set-rt-input{flex:1;min-width:0;display:flex;align-items:center;gap:var(--sp-4);height:var(--ctl-h-lg);padding:0 var(--sp-5);
      background:var(--panel);border:1px solid var(--hair);border-radius:8px;transition:border-color .12s,box-shadow .12s;}
    .set-rt-input:focus-within{border-color:var(--ui-focus);box-shadow:var(--ui-ring);}
    .set-rt-input app-icon{color:var(--ink-4);flex:none;display:inline-flex;}
    .set-rt-input ::ng-deep input{flex:1;min-width:0;width:100%;background:transparent;border:none;outline:none;color:var(--ink);
      font-family:var(--font-ui);padding:0;}
    .set-rt-input ::ng-deep input::placeholder{color:var(--ink-4);}
    .set-rt-act{display:flex;align-items:center;gap:var(--sp-3);flex:none;}
    .set-rt-fail{display:flex;align-items:center;gap:var(--sp-3);color:var(--set-danger);}
    .set-rt-fail app-icon{flex:none;display:inline-flex;}
    .set-rt-cancel ::ng-deep .kj-button{--kj-button-fg:var(--ink-4);--kj-button-font:var(--font-ui);
      --kj-button-margin-left:auto;}
    .set-rt-cancel ::ng-deep .kj-button:hover:not([aria-disabled="true"]){--kj-button-fg:var(--ink-2);}
    `,
  ],
})
export class RuntimeRowComponent {
  // Decorator inputs (not signal `input()`) backed by signals: signal inputs
  // don't wire under raw vitest JIT, while decorator-input bindings do — and the
  // signal backing keeps the detection computed reactive to a tool switch.
  readonly toolId = signal("");
  readonly toolName = signal("");
  @Input({ alias: "toolId" }) set toolIdInput(v: string) {
    this.toolId.set(v);
  }
  @Input({ alias: "toolName" }) set toolNameInput(v: string) {
    this.toolName.set(v);
  }

  private readonly store = inject(SettingsStore);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly bridge = inject(BRIDGE);

  /** This tool's live detection (null until startup detection completes). */
  readonly det = computed(() => this.runtime.detection(this.toolId()));
  readonly isOk = computed(() => this.det()?.status === "ok");

  readonly editing = signal(false);
  readonly draft = signal("");
  readonly checking = signal(false);
  readonly fail = signal<string | null>(null);

  /** The locate/verify editor shows whenever the tool isn't OK, or the user
   *  pressed "Change" on a working one. */
  readonly showEditor = computed(() => !this.isOk() || this.editing());

  readonly statusLabel = computed(() => {
    const d = this.det();
    if (d?.status === "ok") {
      const src = d.source === "manual" ? "manual path" : "on PATH";
      return d.version ? `detected · ${src} · v${d.version}` : `detected · ${src}`;
    }
    if (d?.status === "error") return "found, can’t run";
    return "not installed";
  });

  begin(): void {
    this.draft.set(this.det()?.path ?? "");
    this.fail.set(null);
    this.editing.set(true);
  }
  cancel(): void {
    this.editing.set(false);
    this.fail.set(null);
  }
  onEscape(): void {
    if (this.isOk()) this.cancel();
  }
  onDraft(v: string): void {
    this.draft.set(v);
    this.fail.set(null);
  }

  async verify(): Promise<void> {
    const v = this.draft().trim();
    if (!v) {
      this.fail.set("Enter the full path to the executable.");
      return;
    }
    this.fail.set(null);
    this.checking.set(true);
    try {
      const det = await this.runtime.verifyToolPath(this.toolId(), v);
      if (det.status === "ok") {
        this.store.setMap("toolPath", this.toolId(), v); // persist + use at launch
        this.runtime.setDetection(this.toolId(), det);
        this.editing.set(false);
      } else {
        this.fail.set(det.reason ?? "Couldn’t run that binary.");
      }
    } catch {
      this.fail.set("Verification failed — is the backend running?");
    } finally {
      this.checking.set(false);
    }
  }

  async browse(): Promise<void> {
    try {
      // Open the picker at whatever path is already in the field (or the detected
      // one), so the user lands next to the binary instead of at $HOME.
      const start = this.draft().trim() || this.det()?.path || undefined;
      const f = await this.bridge.pickFile(start);
      if (f) {
        this.draft.set(f);
        this.fail.set(null);
      }
    } catch {
      /* picker unavailable (plain browser) — ignore */
    }
  }

  async revert(): Promise<void> {
    this.store.setMap("toolPath", this.toolId(), null); // clear the override
    this.editing.set(false);
    this.fail.set(null);
    await this.runtime.refreshDetections(); // re-detect on PATH
  }
}
