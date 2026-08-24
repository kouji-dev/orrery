import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ViewEncapsulation,
  computed,
  effect,
  inject,
  isDevMode,
  signal,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectsStore } from "../stores/projects.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { PerfStore, PerfRow, PERF_WINDOW_MS } from "../perf/perf.store";
import { setSchedulerStatsEnabled, terminalSchedulerStats } from "../terminal-output-scheduler";
import { MetricsStore } from "../metrics/metrics.store";
import { TelemetryStore } from "../metrics/telemetry.store";
import { DevPanelStore } from "./dev-panel.store";
import { MERGED_ROOT_KEY, mergeRoots } from "./process-tree-merge";
import { BRIDGE, Commands } from "../data-source/bridge";
import {
  Agent,
  AgentStatus,
  EmitAggRow,
  ProcessNode,
  ProcessTreeSnapshot,
  Project,
  TelemetryTraceState,
} from "../models";
import {
  KjBadgeComponent,
  KjButtonComponent,
  KjEmptyStateComponent,
  KjEmptyStateDescriptionComponent,
  KjEmptyStateIconComponent,
  KjEmptyStateTitleComponent,
  KjOverlayBadgeComponent,
  KjProgressBarComponent,
} from "@kouji-ui/components";
import { DismissDirective } from "../shared/dismiss.directive";

type Sort = { key: string; dir: number };

/**
 * Dev console (floating, FAB-launched). Three tabs:
 *  - **Perf** — live per-command round-trip/exec metrics from {@link PerfStore},
 *    color-coded (green ≤16ms / amber ≤100ms / red >100ms), sortable, with a
 *    recent-calls feed and per-command expand (dev tier only).
 *  - **Agents / Projects** — live in-memory state inspectors.
 *
 * Ported from the Claude Design `devpanel.jsx`; latency colors are scoped under
 * `.dvcon` so `ViewEncapsulation.None` is safe.
 */
@Component({
  selector: "app-dev-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  hostDirectives: [DismissDirective],
  imports: [
    IconComponent,
    StatusDotComponent,
    ToolBadgeComponent,
    KjBadgeComponent,
    KjButtonComponent,
    KjEmptyStateComponent,
    KjEmptyStateDescriptionComponent,
    KjEmptyStateIconComponent,
    KjEmptyStateTitleComponent,
    KjOverlayBadgeComponent,
    KjProgressBarComponent,
  ],
  template: `
    <kj-overlay-badge [kjValue]="alertCount()" [kjHidden]="alertCount() === 0" kjVariant="destructive" [kjDescription]="alertCount() + ' perf alert' + (alertCount() > 1 ? 's' : '')">
      <kj-button kjVariant="ghost" class="fab dvc-fab" [class.on]="open()" [title]="alertCount() ? 'Dev console · ' + alertCount() + ' perf alert' + (alertCount() > 1 ? 's' : '') : 'Dev console'" kjAriaLabel="Dev console" (click)="open.set(!open())">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12h3l2.5-6 4 13 3-9 1.5 2H21" /></svg>
      </kj-button>
    </kj-overlay-badge>

    @if (open()) {
      <section class="corner-panel dvcon" aria-label="Dev console">
        <header class="pane-head dvc-head">
          <span class="dvc-brand"><app-icon name="cpu" size="sm" /></span>
          <div class="dvc-tabs scroll-hide">
            <kj-button kjVariant="ghost" class="dvc-tab" [kjPressed]="tab() === 'perf'" (click)="tab.set('perf')"><app-icon name="spark" size="sm" />Perf</kj-button>
            <kj-button kjVariant="ghost" class="dvc-tab" [kjPressed]="tab() === 'agents'" (click)="tab.set('agents')"><app-icon name="agent" size="sm" />Agents<span class="dvc-cnt">{{ agents().length }}</span></kj-button>
            <kj-button kjVariant="ghost" class="dvc-tab" [kjPressed]="tab() === 'projects'" (click)="tab.set('projects')"><app-icon name="box" size="sm" />Projects<span class="dvc-cnt">{{ projects().length }}</span></kj-button>
            <kj-button kjVariant="ghost" class="dvc-tab" [kjPressed]="tab() === 'resources'" (click)="tab.set('resources')"><app-icon name="cpu" size="sm" />Resources<span class="dvc-cnt">{{ treeProcCount() }}</span></kj-button>
            <kj-button kjVariant="ghost" class="dvc-tab" [kjPressed]="tab() === 'emits'" (click)="tab.set('emits')"><app-icon name="spark" size="sm" />Emits<span class="dvc-cnt">{{ emitRows().length }}</span></kj-button>
          </div>
          <span class="dvc-live on"><span class="dvc-ld"></span>live</span>
          @if (tab() === 'perf') {
            @if (hasCalls()) {
              <kj-button class="dvc-ic" [class.ok]="copied()" (click)="copyPerf()" [title]="copied() ? 'Copied to clipboard' : 'Copy perf data as JSON'"><app-icon [name]="copied() ? 'check' : 'dup'" size="sm" />{{ copied() ? 'Copied' : 'Copy' }}</kj-button>
            }
            <kj-button class="dvc-ic" (click)="perf.clear()" title="Clear counters"><app-icon name="refresh" size="sm" />Reset</kj-button>
          }
          <kj-button kjSize="icon" class="dvc-x" (click)="open.set(false)" title="Close" kjAriaLabel="Close"><app-icon name="x" size="sm" /></kj-button>
        </header>

        <div class="dvc-body">
          <!-- ── PERF ── -->
          @if (tab() === 'perf') {
            @if (hasCalls()) {
              <div class="dvc-scroll">
              <table class="dvc-tbl">
                <thead><tr>
                  @for (c of PCOLS; track c[0]) {
                    <th class="up" [class.srt]="sort().key === c[0]" (click)="clickPerfSort(c[0])">{{ c[1] }}@if (sort().key === c[0]) { <span class="dvc-arr">{{ sort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                  }
                </tr></thead>
                <tbody>
                  @for (s of sortedPerf(); track s.cmd) {
                    <tr class="dvc-row row-hover" [class.open]="openCmd() === s.cmd" [class.stale]="s.stale" (click)="openCmd.set(openCmd() === s.cmd ? null : s.cmd)">
                      <td><span class="dvc-lead"><app-icon class="dvc-tw" name="chevron" size="sm" /><span class="dvc-nm trunc">{{ s.cmd }}</span>@if (s.stale) { <span class="dvc-stale">(stale)</span> }</span></td>
                      <td class="tnum" style="color:var(--ink-2)">{{ s.calls10s }}</td>
                      <td [class]="'dvc-lat ' + lat(s.avgRt) + ' tnum'"><span class="v">{{ ms(s.avgRt) }}</span></td>
                      <td [class]="'dvc-lat ' + lat(s.avgExec) + ' tnum'"><span class="v">{{ ms(s.avgExec) }}</span></td>
                      <td class="tnum" style="color:var(--ink-2)">{{ ovh(s.overhead) }}</td>
                      <td [class]="'dvc-lat ' + lat(s.p95Rt) + ' tnum'"><span class="v">{{ ms(s.p95Rt) }}</span></td>
                      <td [class]="'dvc-lat ' + lat(s.maxRt) + ' tnum'"><span class="v">{{ ms(s.maxRt) }}</span></td>
                      <td [class]="'dvc-err ' + (s.errPct > 0 ? 'bad' : 'zero') + ' tnum'"><span class="v">{{ s.errPct.toFixed(1) }}%</span></td>
                      <td class="dvc-spk">
                        <svg [attr.width]="58" [attr.height]="16" style="display:block">
                          <polyline [attr.points]="sparkPoints(s.hist)" fill="none" [attr.stroke]="latColor(lat(s.avgRt))" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round" opacity="0.9" />
                        </svg>
                      </td>
                    </tr>
                    @if (openCmd() === s.cmd) {
                      <tr class="dvc-detail"><td [attr.colspan]="PCOLS.length"><div class="dvc-din">
                        <div class="dvc-dh"><span class="up lbl">recent · {{ s.cmd }}</span><span class="dvc-ds tnum"><span>p95 <b>{{ ms(s.p95Rt) }}</b></span><span>max <b>{{ ms(s.maxRt) }}</b></span><span>overhead <b>{{ ovh(s.overhead) }}</b></span></span></div>
                        <div class="dvc-calls">
                          @for (c of s.recent; track $index) {
                            <div class="dvc-cr"><span class="ts tnum">{{ clock(c.ts) }}</span><span class="cm trunc">{{ s.cmd }}</span><span [class]="'du ' + lat(c.ms) + ' tnum'">{{ ms(c.ms) }}</span><span [class]="'dvc-dot ' + (c.ok ? 'ok' : 'er')"></span></div>
                          }
                        </div>
                      </div></td></tr>
                    }
                  }
                </tbody>
              </table>
              </div>
            } @else {
              <kj-empty-state style="flex:1">
                <kj-empty-state-icon><span class="dvc-ring"><app-icon name="spark" /></span></kj-empty-state-icon>
                <kj-empty-state-title>No calls yet</kj-empty-state-title>
                <kj-empty-state-description>Invoke metrics populate as the frontend dispatches Tauri commands. Counters reset on a rolling 10-second window.</kj-empty-state-description>
                <span class="dvc-hint"><span class="dvc-ld"></span>listening for invokes…</span>
              </kj-empty-state>
            }
            @if (hasCalls()) {
              <div class="dvc-feed">
                <kj-button kjVariant="ghost" class="dvc-fh" [class.open]="feedOpen()" (click)="feedOpen.set(!feedOpen())">
                  <app-icon class="dvc-tw" [class.open]="feedOpen()" name="chevron" size="sm" />
                  <span class="up lbl">Recent calls</span>
                  <app-icon name="timeline" size="sm" [color]="'var(--ink-4)'" />
                  <span class="ct">last {{ feed().length }}</span>
                </kj-button>
                @if (feedOpen()) {
                  <div class="dvc-fl">
                    @for (f of feed(); track f.ts + f.cmd) {
                      <div class="dvc-fr"><span class="ts tnum">{{ clock(f.ts) }}</span><span [class]="'dvc-dot ' + (f.ok ? 'ok' : 'er')" style="justify-self:start"></span><span class="cm trunc">{{ f.cmd }}</span><span [class]="'du ' + lat(f.ms) + ' tnum'">{{ ms(f.ms) }}</span><span class="ix tnum">{{ f.ok ? 'ok' : 'err' }}</span></div>
                    }
                  </div>
                }
              </div>
            }
          }

          <!-- ── AGENTS ── -->
          @if (tab() === 'agents') {
            <div class="dvc-scroll">
            <table class="dvc-tbl">
              <thead><tr>
                @for (c of ACOLS; track c[0]) {
                  <th class="up" [class.srt]="aSort().key === c[0]" (click)="clickASort(c[0])">{{ c[1] }}@if (aSort().key === c[0]) { <span class="dvc-arr">{{ aSort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                }
              </tr></thead>
              <tbody>
                @for (ag of sortedAgents(); track ag.id) {
                  <tr class="dvc-row row-hover" [class.open]="openAg() === ag.id" (click)="openAg.set(openAg() === ag.id ? null : ag.id)">
                    <td><span class="dvc-lead"><app-icon class="dvc-tw" name="chevron" size="sm" /><span class="dvc-id">{{ ag.id.slice(0, 8) }}</span><span class="dvc-nm trunc">{{ ag.name }}</span>@if (attn(ag)) { <span class="dvc-needs" title="needs you"></span> }</span></td>
                    <td><span class="dvc-stat"><span class="lb" [style.color]="statusColor(ag.status)">{{ ag.status }}</span><app-status-dot [status]="ag.status" /></span></td>
                    <td><span class="dvc-tool" style="vertical-align:middle" [title]="ag.tool + ' · ' + ag.model"><app-tool-badge [tool]="ag.tool" [size]="17" /></span></td>
                    <td><kj-badge variant="outline"><span class="dvc-pn trunc">{{ proj(ag.projectId).name }}</span><span class="dvc-pi" [style.background]="'color-mix(in oklch,' + proj(ag.projectId).color + ',transparent 82%)'"><app-icon size="md" [name]="proj(ag.projectId).icon" [color]="proj(ag.projectId).color" /></span></kj-badge></td>
                    <td><span class="dvc-branch trunc">{{ ag.branch.replace('agent/', '') }}</span></td>
                    <td class="tnum">{{ ag.commits }}</td>
                    <td class="tnum" style="color:var(--ink-3)">{{ elapsed(ag) ? fmtDur(elapsed(ag)) : '—' }}</td>
                    <td class="tnum"><kj-progress-bar class="dvc-mtr" [kjValue]="pct(ag.progress)" kjAriaLabel="Agent progress" [style.--kj-progress-bar-fill]="statusColor(ag.status)" style="--kj-progress-bar-track:var(--hair);--kj-progress-bar-radius:3px;--kj-progress-bar-height:var(--sp-2)" />{{ pct(ag.progress) }}%</td>
                  </tr>
                  @if (openAg() === ag.id) {
                    <tr class="dvc-detail"><td [attr.colspan]="ACOLS.length"><div class="dvc-din">
                      <div class="dvc-task">{{ ag.task }}</div>
                      @if (attn(ag) && note(ag)) { <div class="dvc-attn"><app-icon name="flag" size="sm" />{{ note(ag) }}</div> }
                      <div class="dvc-kv">
                        <div class="r"><span class="k">id</span><span class="vv trunc"><b>{{ ag.id }}</b></span></div>
                        <div class="r"><span class="k">tool</span><span class="vv trunc">{{ ag.tool }} · {{ ag.model }}{{ ag.effort ? ' · ' + ag.effort : '' }}</span></div>
                        <div class="r"><span class="k">status</span><span class="vv trunc">{{ ag.status }}{{ ag.pending.length ? ' · ' + ag.pending.length + ' pending' : '' }}</span></div>
                        <div class="r"><span class="k">branch</span><span class="vv trunc">{{ ag.branch }}</span></div>
                        <div class="r"><span class="k">worktree</span><span class="vv trunc">{{ ag.worktree }}</span></div>
                        <div class="r"><span class="k">base</span><span class="vv trunc">{{ ag.base }}</span></div>
                        <div class="r"><span class="k">commits</span><span class="vv trunc">{{ ag.commits }}</span></div>
                        <div class="r"><span class="k">progress</span><span class="vv trunc">{{ pct(ag.progress) }}%</span></div>
                      </div>
                    </div></td></tr>
                  }
                }
              </tbody>
            </table>
            </div>
          }

          <!-- ── PROJECTS ── -->
          @if (tab() === 'projects') {
            <div class="dvc-scroll">
            <table class="dvc-tbl">
              <thead><tr>
                @for (c of PRCOLS; track c[0]) {
                  <th class="up" [class.srt]="pSort().key === c[0]" (click)="clickPSort(c[0])">{{ c[1] }}@if (pSort().key === c[0]) { <span class="dvc-arr">{{ pSort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                }
              </tr></thead>
              <tbody>
                @for (p of sortedProjects(); track p.id) {
                  <tr class="dvc-row row-hover" [class.open]="openPr() === p.id" (click)="openPr.set(openPr() === p.id ? null : p.id)">
                    <td><span class="dvc-lead"><app-icon class="dvc-tw" name="chevron" size="sm" /><span class="dvc-pi" style="width:var(--sp-7);height:var(--sp-7);border-radius:5px" [style.background]="'color-mix(in oklch,' + p.color + ',transparent 82%)'"><app-icon size="md" [name]="p.icon" [color]="p.color" /></span><span class="dvc-nm trunc" [style.color]="p.color">{{ p.name }}</span></span></td>
                    <td class="dvc-id tnum">{{ p.id.slice(0, 8) }}</td>
                    <td class="tnum" style="color:var(--ink-3)">{{ p.head ?? '—' }}</td>
                    <td style="color:var(--ink-3)">{{ p.branch ?? '—' }}</td>
                    <td class="tnum">{{ (p.branches ?? []).length }}</td>
                    <td class="tnum">{{ agentsIn(p.id).length }}@if (needsIn(p.id) > 0) { <span style="color:var(--st-blocked);font-weight:var(--fw-strong)"> {{ needsIn(p.id) }}!</span> }</td>
                    <td class="tnum">{{ (p.files ?? []).length }}</td>
                  </tr>
                  @if (openPr() === p.id) {
                    <tr class="dvc-detail"><td [attr.colspan]="PRCOLS.length"><div class="dvc-din">
                      <div class="dvc-kv">
                        <div class="r"><span class="k">id</span><span class="vv trunc"><b>{{ p.id }}</b></span></div>
                        <div class="r"><span class="k">path</span><span class="vv trunc">{{ p.path }}</span></div>
                        <div class="r"><span class="k">repo</span><span class="vv trunc">{{ p.repo ?? '—' }}</span></div>
                        <div class="r"><span class="k">head</span><span class="vv trunc">{{ p.head ?? '—' }} ({{ p.branch ?? '—' }})</span></div>
                        <div class="r"><span class="k">hasGit</span><span class="vv trunc">{{ p.hasGit }}</span></div>
                        <div class="r"><span class="k">folderExists</span><span class="vv trunc">{{ p.folderExists }}</span></div>
                      </div>
                      @if ((p.branches ?? []).length) {
                        <div><div class="up dvc-sl">branches · {{ (p.branches ?? []).length }}</div><div style="display:flex;flex-wrap:wrap;gap:var(--sp-3)">@for (b of p.branches ?? []; track b) { <span class="chip"><app-icon size="md" name="branch" />{{ b }}</span> }</div></div>
                      }
                      <div><div class="up dvc-sl">agents · {{ agentsIn(p.id).length }}</div><div class="dvc-files">@for (a of agentsIn(p.id); track a.id) { <div class="dvc-frow" style="grid-template-columns:9px 1fr auto"><app-status-dot [status]="a.status" /><span class="dvc-fp trunc">{{ a.id.slice(0, 8) }} · {{ a.name }}</span><span class="chip"><app-tool-badge [tool]="a.tool" [size]="13" />{{ a.status }}</span></div>} @empty { <div class="dvc-dim">no agents</div> }</div></div>
                    </div></td></tr>
                  }
                }
              </tbody>
            </table>
            </div>
          }

          <!-- ── RESOURCES (merged: gauges + the A7.7 recursive process tree,
               rooted at a synthetic "Orrery App" row that sums EVERYTHING —
               rust core, webview family, every agent subtree) ── -->
          @if (tab() === 'resources') {
            @if (procs().length) {
              <div class="dvc-res-top">
                <div class="dvc-gauge">
                  <div class="g-top">
                    <span class="up g-lab"><app-icon name="cpu" size="sm" />CPU</span>
                    <span class="g-val tnum">{{ totalCpu().toFixed(1) }}<small>%</small></span>
                  </div>
                  <kj-progress-bar kjSize="sm" [kjValue]="barClamp(totalCpu())" kjAriaLabel="CPU usage" [style.--kj-progress-bar-fill]="latColor(gaugeCpuC())" style="--kj-progress-bar-track:var(--hair);--kj-progress-bar-radius:4px;--kj-progress-bar-height:var(--sp-3)" />
                  <span class="g-sub tnum">{{ coresUsed() }} of {{ cores() }} cores · {{ agentProcCount() }} agent {{ agentProcCount() === 1 ? 'subtree' : 'subtrees' }}</span>
                </div>
                <div class="dvc-gauge">
                  <div class="g-top">
                    <span class="up g-lab"><app-icon name="database" size="sm" />Memory</span>
                    <span class="g-val tnum">{{ memGb() }}<small> GB</small></span>
                  </div>
                  <kj-progress-bar kjSize="sm" [kjValue]="barClamp(memPct())" kjAriaLabel="Memory usage" [style.--kj-progress-bar-fill]="latColor(gaugeMemC())" style="--kj-progress-bar-track:var(--hair);--kj-progress-bar-radius:4px;--kj-progress-bar-height:var(--sp-3)" />
                  <span class="g-sub tnum">{{ memPct().toFixed(1) }}% of {{ sysGb() }} GB</span>
                </div>
              </div>
            }
            @if (tree()) {
              <div class="dvc-split">
                <span class="tnum" style="color:var(--ink-2)">Orrery <b style="color:var(--ink)">{{ fmtMem(orreryPriv()) }}</b></span>
                <span style="color:var(--ink-4)">·</span>
                <span class="tnum" style="color:var(--ink-2)">agents <b style="color:var(--ink)">{{ fmtMem(agentsPriv()) }}</b></span>
                <kj-badge variant="outline" style="--kj-badge-font-size:var(--fs-meta)">private working set</kj-badge>
                <span style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4)">job-object accounting · descendants rediscovered at slower cadence</span>
              </div>
              @for (al of treeAlerts(); track al.id) {
                <div class="dvc-palert">
                  <app-icon name="flag" size="sm" [color]="'var(--st-blocked)'" />
                  <span>agent <b>{{ al.label }}</b> subtree <b>{{ fmtMem(al.priv) }}</b> — {{ fmtMem(al.childPriv) }} of it is {{ al.childNames }} it started</span>
                </div>
              }
              <div class="dvc-scroll">
              <table class="dvc-tbl">
                <thead><tr>
                  <th style="cursor:default">Process</th>
                  <th style="cursor:default">PID</th>
                  <th style="cursor:default">CPU</th>
                  <th style="cursor:default">Private</th>
                  <th style="cursor:default">Subtree</th>
                </tr></thead>
                <tbody>
                  @for (r of treeRows(); track r.key) {
                    <tr class="dvc-row row-hover" style="cursor:default" [style.opacity]="r.n.excluded ? 0.55 : 1">
                      <td><span class="dvc-lead" [style.padding-left]="r.n.children.length ? (r.depth * 15) + 'px' : 'calc(' + (r.depth * 15) + 'px + var(--sp-5) + var(--sp-4))'">
                        @if (r.n.children.length) {
                          <kj-button kjSize="icon" kjVariant="ghost" class="dvc-twbtn" (click)="toggleNode(r.key)" kjAriaLabel="Toggle subtree">
                            <app-icon class="dvc-tw" [class.open]="expanded(r.key)" name="chevron" size="sm" />
                          </kj-button>
                        }
                        @if (r.agentRoot && agentOf(r.rootId); as ag) {
                          <span class="dvc-kind" style="background:transparent"><app-tool-badge [tool]="ag.tool" [size]="15" /></span>
                        } @else {
                          <span class="dvc-pdot" [style.background]="nodeDot(r.n)"></span>
                        }
                        <span class="dvc-nm trunc" [style.color]="notOurs(r.n) ? 'var(--ink-3)' : 'var(--ink)'" [style.font-weight]="r.depth === 0 ? 600 : null">{{ r.n.name }}</span>
                        @if (r.n.note) { <span style="font-size:var(--fs-meta);color:var(--ink-4);flex:none">{{ r.n.note }}</span> }
                        @if (r.n.detached) { <kj-badge variant="outline" fg="var(--lat-a)" style="--kj-badge-font-size:var(--fs-badge)">detached · found via job object</kj-badge> }
                      </span></td>
                      <td class="tnum" style="color:var(--ink-4)">{{ r.n.pid || '—' }}</td>
                      <td class="tnum" [style.color]="r.n.cpu > 60 ? 'var(--lat-r)' : r.n.cpu > 25 ? 'var(--lat-a)' : 'var(--ink-2)'">{{ r.n.cpu.toFixed(1) }}%</td>
                      <td class="tnum" [style.color]="r.n.privBytes > 1073741824 ? 'var(--lat-r)' : 'var(--ink)'">{{ fmtMem(r.n.privBytes) }}</td>
                      <td class="tnum" [style.color]="r.n.children.length ? 'var(--ink-2)' : 'var(--ink-4)'">{{ r.n.children.length ? fmtMem(r.n.subtreePrivBytes) : '—' }}</td>
                    </tr>
                  }
                </tbody>
              </table>
              </div>
            } @else {
              <kj-empty-state style="flex:1">
                <kj-empty-state-icon><span class="dvc-ring"><app-icon name="cpu" /></span></kj-empty-state-icon>
                <kj-empty-state-title>No process tree yet</kj-empty-state-title>
                <kj-empty-state-description>The process sampler only runs while this tab is open (known pids + their discovered descendants — never a machine-wide sweep per poll).</kj-empty-state-description>
                <span class="dvc-hint"><span class="dvc-ld"></span>sampling…</span>
              </kj-empty-state>
            }
          }

          <!-- ── EMITS (A0.7 phase 1 aggregate) ── -->
          @if (tab() === 'emits') {
            <div class="dvc-split">
              <span class="tnum" style="color:var(--ink-2)">{{ emitRows().length }} event names · {{ fmtBytes(emitTotal()) }} emitted this session</span>
              <kj-badge variant="outline" fg="var(--lat-g)" style="--kj-badge-font-size:var(--fs-meta)">aggregate always on</kj-badge>
              <span style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-4)">
                @if (telemetry.traceActive()) {
                  <span class="tnum" style="display:flex;align-items:center;gap:var(--sp-3);color:var(--lat-r)">
                    <span class="dvc-recdot"></span>recording{{ traceLine() }}
                  </span>
                }
                <kj-button kjVariant="ghost" class="dvc-ic" [style.color]="telemetry.traceActive() ? 'var(--lat-r)' : null" (click)="toggleTrace()" [title]="telemetry.traceActive() ? 'Stop the raw emit trace' : 'Raw trace: one ts·name·key·bytes line per emit — auto-off after 30min or 200MB'">
                  <app-icon [name]="telemetry.traceActive() ? 'stop' : 'play'" size="sm" />{{ telemetry.traceActive() ? 'Stop raw trace' : 'Start raw trace' }}
                </kj-button>
              </span>
            </div>
            @if (emitRows().length) {
              <div class="dvc-scroll">
              <table class="dvc-tbl">
                <thead><tr>
                  @for (c of ECOLS; track c[0]) {
                    <th class="up" [class.srt]="eSort().key === c[0]" (click)="clickESort(c[0])">{{ c[1] }}@if (eSort().key === c[0]) { <span class="dvc-arr">{{ eSort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                  }
                </tr></thead>
                <tbody>
                  @for (r of sortedEmits(); track r.name) {
                    <tr class="dvc-row row-hover" style="cursor:default">
                      <td><span class="dvc-lead"><span class="dvc-nm trunc">{{ r.name }}</span></span></td>
                      <td class="tnum" style="color:var(--ink-2)">{{ r.count.toLocaleString() }}</td>
                      <td class="tnum" [style.color]="r.totalBytes > 10000000 ? 'var(--lat-r)' : 'var(--ink-2)'">{{ fmtBytes(r.totalBytes) }}</td>
                      <td class="tnum" style="color:var(--ink-4)">{{ fmtBytes(r.p50Bytes) }}</td>
                      <td class="tnum" style="color:var(--ink-4)">{{ fmtBytes(r.p95Bytes) }}</td>
                      <td class="tnum" style="color:var(--ink-2)">{{ r.peakPerSec }}</td>
                      <td><kj-badge variant="outline" [fg]="clsColor(r)" [style.--kj-badge-border-color]="'color-mix(in oklch,' + clsColor(r) + ',transparent 60%)'" style="--kj-badge-font-size:var(--fs-meta)">{{ suggestClass(r) }}</kj-badge></td>
                    </tr>
                  }
                </tbody>
              </table>
              </div>
            } @else {
              <kj-empty-state style="flex:1">
                <kj-empty-state-icon><span class="dvc-ring"><app-icon name="spark" /></span></kj-empty-state-icon>
                <kj-empty-state-title>No emits recorded yet</kj-empty-state-title>
                <kj-empty-state-description>Every backend event goes through the emit_tracked funnel — rows appear as soon as anything is pushed to the UI.</kj-empty-state-description>
                <span class="dvc-hint"><span class="dvc-ld"></span>listening…</span>
              </kj-empty-state>
            }
          }
        </div>

        <footer class="dvc-foot">
          @if (tab() === 'perf') {
            <span class="dvc-leg"><span class="dvc-sw g"></span>≤16ms</span>
            <span class="dvc-leg"><span class="dvc-sw a"></span>16–100ms</span>
            <span class="dvc-leg"><span class="dvc-sw r"></span>&gt;100ms · err</span>
            <span class="tnum" style="margin-left:auto">{{ perfSummary() }}</span>
            <span class="tnum" title="terminal write scheduler: queued chars · peak · dropped backlogs">term {{ termStats().queuedChars }} · pk {{ termStats().peakQueuedChars }} · drop {{ termStats().droppedBacklogs }}</span>
            @if (ptyLine()) {
              <span class="tnum" title="batched PTY output: throughput · emit rate · avg batch size">{{ ptyLine() }}</span>
            }
          }
          @if (tab() === 'agents') {
            <span class="tnum">{{ agents().length }} agents · <span style="color:var(--st-running)">{{ cnt('running') }} running</span> · <span style="color:var(--st-blocked)">{{ cnt('blocked') }} need you</span> · {{ cnt('waiting') + cnt('queued') }} waiting · {{ cnt('done') }} done</span>
            <span class="tnum" style="margin-left:auto">live state</span>
          }
          @if (tab() === 'projects') {
            <span class="tnum">{{ projects().length }} projects · {{ agents().length }} worktrees</span>
            <span class="tnum" style="margin-left:auto">live state</span>
          }
          @if (tab() === 'resources') {
            <span>subtree totals sum private working set — RSS double-counts shared pages and would overstate</span>
            <span class="tnum" style="margin-left:auto">{{ totalCpu().toFixed(1) }}% / {{ cores() }} cores · {{ memGb() }} GB · {{ treeProcCount() }} procs · refreshed {{ treeAge() }}</span>
          }
          @if (tab() === 'emits') {
            <span>app-data/telemetry/emit-summary-…json</span>
            <span style="color:var(--lat-g)">names · keys · byte counts only — payload contents are never written</span>
            <span class="tnum" style="margin-left:auto">≤50 MB / 7 days · daily rotation</span>
          }
        </footer>
      </section>
    }
  `,
  styles: [
    `
.dvcon,.dvc-fab{--lat-g:var(--sem-add);--lat-a:var(--sem-attn);--lat-r:var(--sem-del);
  --lat-g-bg:color-mix(in oklch,var(--sem-add),transparent 90%);--lat-a-bg:color-mix(in oklch,var(--sem-attn),transparent 86%);--lat-r-bg:color-mix(in oklch,var(--sem-del),transparent 82%);
  --lat-r-ring:rgba(255,93,122,.4);}
/* FAB skin, the corner-panel box and its entrance are shared recipes in
   styles.css (.fab / .corner-panel / @keyframes pop-up) — the copies here and
   in tweaks-panel were identical down to a second keyframes block under
   another name. Only the rail slot and the console's own size stay local. */
.dvcon{bottom:92px;z-index:91;width: round(calc(860px * var(--density)), 1px);max-width:calc(100vw - 32px);
  max-height:calc(100vh - 148px);display:flex;flex-direction:column;}
.dvcon .dvc-head{gap:var(--sp-5);padding:var(--sp-4) var(--sp-5) var(--sp-4) var(--sp-6);background:linear-gradient(180deg,var(--panel-3),var(--panel));}
.dvc-brand{display:flex;align-items:center;color:var(--ui-ink);}
.dvc-tabs{display:flex;gap:var(--sp-1);padding:var(--sp-1);background:var(--panel-2);border:1px solid var(--hair);border-radius:8px;min-width:0;flex:0 1 auto;overflow-x:auto;overflow-y:hidden;}
/* look verified against design/app.html:9626 and kept. The SELECTED skin is
   the global .kj-button[aria-pressed="true"] rule (same panel-3 + hairline
   ring), so the tabs run on [kjPressed] now and the rest-state skin is
   scoped to :not([aria-pressed]) — otherwise it would out-specify it. */
.dvc-tab .kj-button{flex:none;display:inline-flex;align-items:center;gap:var(--sp-3);height:var(--ctl-h);font-family:var(--font-ui);font-weight:var(--fw-medium);border:none;border-radius:6px;padding:0 var(--sp-5);cursor:pointer;transition:all .12s;}
.dvc-tab .kj-button:not([aria-pressed="true"]){--kj-button-bg:transparent;--kj-button-fg:var(--ink-3);box-shadow:none;}
.dvc-tab .kj-button:not([aria-pressed="true"]):hover{--kj-button-fg:var(--ink-2);--kj-button-bg:transparent;}
.dvc-tab [aria-pressed="true"] svg{color:var(--ui-ink);}
.dvc-tab svg{width:var(--sp-6);height:var(--sp-6);color:var(--ink-4);}
.dvc-cnt{display:inline-flex;align-items:center;justify-content:center;height:var(--sp-7);font-size:var(--fs-meta);padding:0 var(--sp-2);border-radius:999px;background:var(--panel);border:1px solid var(--hair);color:var(--ink-3);min-width:16px;}
.dvc-tab [aria-pressed="true"] .dvc-cnt{color:var(--ink-2);border-color:var(--hair-2);}
.dvc-live{display:inline-flex;align-items:center;gap:var(--sp-3);font-size:var(--fs-meta);color:var(--ink-3);margin-right:auto;}
.dvc-ld{width:var(--sp-3);height:var(--sp-3);border-radius:50%;background:#35e0a1;position:relative;}
.dvc-ld::after{content:"";position:absolute;inset:-3px;border-radius:50%;background:inherit;opacity:.4;filter:blur(2px);}
.dvc-live.on .dvc-ld{animation:dvcblink 1.5s ease-in-out infinite;}
@keyframes dvcblink{0%,100%{opacity:1;}50%{opacity:.35;}}
.dvc-ic .kj-button{display:inline-flex;align-items:center;gap:var(--sp-3);height:auto;font-family:var(--font-ui);font-weight:var(--fw-medium);color:var(--ink-2);background:transparent;border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-2) var(--sp-4);cursor:pointer;transition:all .12s;}
.dvc-ic .kj-button:hover{color:var(--ink);background:var(--panel-3);border-color:var(--hair-2);}
.dvc-ic.ok .kj-button{color:var(--lat-g);border-color:color-mix(in oklch,var(--lat-g),transparent 55%);}
.dvc-ic svg{width:var(--sp-6);height:var(--sp-6);}
.dvc-x .kj-button{border:none;height:auto;padding:var(--sp-2);color:var(--ink-3);background:transparent;border-radius:var(--r-sm);cursor:pointer;display:inline-flex;}
.dvc-x .kj-button:hover{color:var(--ink);background:var(--panel-3);}
.dvc-x svg{width:var(--sp-6);height:var(--sp-6);}
.dvc-body{flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden;}
.dvc-scroll{flex:1;min-height:0;overflow:auto;}
.dvc-tbl{min-width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums;}
.dvc-tbl thead th{position:sticky;top:0;z-index:2;background:var(--panel-2);border-bottom:1px solid var(--hair-2);font-weight:var(--fw-medium);color:var(--ink-3);padding:var(--sp-4) var(--sp-4);text-align:right;white-space:nowrap;cursor:pointer;user-select:none;transition:color .12s,background .12s;}
.dvc-tbl thead th:first-child{text-align:left;padding-left:var(--sp-6);}
.dvc-tbl thead th:last-child{padding-right:var(--sp-6);}
.dvc-tbl thead th:hover{color:var(--ink-2);background:var(--panel-3);}
.dvc-tbl thead th.srt{color:var(--ink);}
.dvc-arr{display:inline-block;width:9px;margin-left:var(--sp-1);color:var(--ui-ink);font-size:var(--fs-badge);vertical-align:middle;}
.dvc-tbl tbody td{padding:0 var(--sp-4);height:var(--ctl-h-lg);border-bottom:1px solid var(--hair);text-align:right;white-space:nowrap;color:var(--ink-2);}
.dvc-tbl tbody td:first-child{text-align:left;padding-left:var(--sp-6);}
.dvc-tbl tbody td:last-child{padding-right:var(--sp-6);}
.dvc-row{cursor:pointer;transition:background .1s;}
.dvc-row.open{background:var(--panel-2);}
.dvc-row.stale{opacity:.45;}
.dvc-stale{color:var(--ink-4);flex:none;}
.dvc-tw{flex:none;color:var(--ink-4);transition:transform .15s;}
.dvc-row.open .dvc-tw{transform:rotate(90deg);color:var(--ui-ink);}
.dvc-lead{display:flex;align-items:center;gap:var(--sp-4);color:var(--ink);font-weight:var(--fw-medium);}
.dvc-id{color:var(--ink-4);font-weight:var(--fw-normal);}
.dvc-lat .v{display:inline-block;padding:var(--sp-1) var(--sp-3);border-radius:5px;font-weight:var(--fw-medium);}
.dvc-lat.g .v{color:var(--lat-g);}
.dvc-lat.a .v{color:var(--lat-a);background:var(--lat-a-bg);}
.dvc-lat.r .v{color:var(--lat-r);background:var(--lat-r-bg);font-weight:var(--fw-medium);box-shadow:inset 0 0 0 1px var(--lat-r-ring);}
.dvc-lat.na .v{color:var(--ink-4);}
.dvc-dim{color:var(--ink-4);}
.dvc-err .v{display:inline-block;padding:var(--sp-1) var(--sp-3);border-radius:5px;}
.dvc-err.zero .v{color:var(--ink-4);}
.dvc-err.bad .v{color:var(--lat-r);background:var(--lat-r-bg);font-weight:var(--fw-medium);box-shadow:inset 0 0 0 1px var(--lat-r-ring);}
.dvc-spk{text-align:right;padding-right:var(--sp-6);}
.dvc-spk svg{display:inline-block;vertical-align:middle;}
.dvc-stat{display:inline-flex;align-items:center;gap:var(--sp-3);justify-content:flex-end;}
.dvc-needs{width:var(--sp-2);height:var(--sp-2);border-radius:50%;background:var(--st-blocked);flex:none;}
.dvc-tool{width:17px;height:17px;flex:none;border-radius:4px;display:inline-grid;place-items:center;}
.dvc-pi{width:var(--sp-7);height:var(--sp-7);border-radius:4px;display:grid;place-items:center;flex:none;}
.dvc-pn{max-width: round(calc(96px * var(--density)), 1px);}
.dvc-mtr .kj-progress-bar{display:inline-block;width:46px;vertical-align:middle;margin-right:var(--sp-3);}
.dvc-branch{color:var(--ink-3);max-width: round(calc(120px * var(--density)), 1px);display:inline-block;vertical-align:bottom;}
.dvc-detail>td{padding:0;border-bottom:1px solid var(--hair);background:var(--bg);}
.dvc-din{padding:var(--sp-6) var(--sp-6) var(--sp-6) var(--sp-11);display:flex;flex-direction:column;gap:var(--sp-5);animation:dvcexp .22s ease;}
@keyframes dvcexp{from{opacity:0;transform:translateY(-3px);}to{opacity:1;transform:none;}}
.dvc-kv{display:grid;grid-template-columns:repeat(auto-fill,minmax(210px,1fr));gap:var(--sp-2) var(--sp-8);}
.dvc-kv .r{display:flex;gap:var(--sp-4);align-items:baseline;}
.dvc-kv .k{color:var(--ink-4);min-width: round(calc(74px * var(--density)), 1px);}
.dvc-kv .vv{color:var(--ink-2);}
.dvc-kv .vv b{color:var(--ink);font-weight:var(--fw-medium);}
.dvc-sl{color:var(--ink-3);margin-bottom:var(--sp-1);}
.dvc-files{display:flex;flex-direction:column;gap:1px;}
.dvc-frow{display:grid;grid-template-columns:16px 1fr auto;gap:var(--sp-4);align-items:center;padding:var(--sp-1) 0;color:var(--ink-2);}
.dvc-task{color:var(--ink-2);line-height:1.5;}
.dvc-attn{display:inline-flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-4);border-radius:6px;color:var(--lat-r);background:var(--lat-r-bg);border:1px solid var(--lat-r-ring);align-self:flex-start;}
.dvc-calls{display:flex;flex-direction:column;gap:1px;}
.dvc-cr{display:grid;grid-template-columns:84px 1fr auto 14px;align-items:center;gap:var(--sp-5);padding:var(--sp-1) 0;color:var(--ink-2);}
.dvc-cr .ts{color:var(--ink-4);} .dvc-cr .cm{color:var(--ink-3);}
.dvc-cr .du{text-align:right;font-weight:var(--fw-medium);}
.dvc-cr .du.g{color:var(--lat-g);} .dvc-cr .du.a{color:var(--lat-a);} .dvc-cr .du.r{color:var(--lat-r);}
.dvc-dot{width:var(--sp-3);height:var(--sp-3);border-radius:50%;justify-self:center;}
.dvc-dot.ok{background:var(--lat-g);} .dvc-dot.er{background:var(--lat-r);box-shadow:0 0 0 2px var(--lat-r-bg);}
.dvc-dh{display:flex;align-items:center;gap:var(--sp-5);color:var(--ink-3);}
.dvc-ds{display:flex;gap:var(--sp-6);margin-left:auto;color:var(--ink-2);}
.dvc-ds b{color:var(--ink);font-weight:var(--fw-medium);}
.dvc-feed{flex:none;border-top:1px solid var(--hair-2);}
.dvc-fh .kj-button{display:flex;align-items:center;gap:var(--sp-4);width:100%;height:auto;border-radius:0;padding:var(--sp-4) var(--sp-6);background:var(--panel-2);border:none;border-bottom:1px solid var(--hair);cursor:pointer;color:var(--ink-3);font-family:var(--font-mono);font-weight:var(--fw-normal);text-align:left;}
.dvc-fh .kj-button:hover{color:var(--ink-2);}
.dvc-fh .dvc-tw.open{transform:rotate(90deg);color:var(--ui-ink);}
.dvc-fh .lbl{color:var(--ink-3);}
.dvc-fh .ct{color:var(--ink-4);margin-left:auto;}
.dvc-fl{padding:var(--sp-2) var(--sp-6) var(--sp-6);max-height: round(calc(240px * var(--density)), 1px);overflow-y:auto;}
.dvc-fr{display:grid;grid-template-columns:90px 16px 1fr auto 22px;align-items:center;gap:var(--sp-5);padding:var(--sp-2) 0;border-bottom:1px solid var(--hair);color:var(--ink-2);}
.dvc-fr:last-child{border-bottom:none;}
.dvc-fr .ts{color:var(--ink-4);} .dvc-fr .cm{color:var(--ink);}
.dvc-fr .du{text-align:right;font-weight:var(--fw-medium);}
.dvc-fr .du.g{color:var(--lat-g);} .dvc-fr .du.a{color:var(--lat-a);} .dvc-fr .du.r{color:var(--lat-r);}
.dvc-fr .ix{color:var(--ink-4);text-align:right;font-size:var(--fs-meta);}
.dvc-ring{width:46px;height:46px;border-radius:50%;border:1.5px dashed var(--hair-2);display:grid;place-items:center;color:var(--ink-4);}
.dvcon kj-empty-state{--kj-empty-state-gap:var(--sp-6);--kj-empty-state-padding-y:var(--sp-11);--kj-empty-state-padding-x:var(--sp-8);--kj-empty-state-fg:var(--ink-2);--kj-empty-state-muted-fg:var(--ink-4);--kj-empty-state-title-size:var(--fs-lg);--kj-empty-state-desc-size:var(--fs-sm);--kj-empty-state-max-width:none;}
.dvcon .kj-empty-state-description{max-width: round(calc(300px * var(--density)), 1px);line-height:1.55;}
.dvc-hint{display:inline-flex;align-items:center;gap:var(--sp-3);color:var(--ink-3);}
.dvc-hint .dvc-ld{animation:dvcblink 1.5s ease-in-out infinite;}
.dvc-res-top{flex:none;display:grid;grid-template-columns:1fr 1fr;gap:var(--sp-6);padding:var(--sp-6);border-bottom:1px solid var(--hair);}
.dvc-gauge{background:var(--panel-2);border:1px solid var(--hair);border-radius:10px;padding:var(--sp-5) var(--sp-6);display:flex;flex-direction:column;gap:var(--sp-3);}
.dvc-gauge .g-top{display:flex;align-items:baseline;gap:var(--sp-4);}
.dvc-gauge .g-lab{color:var(--ink-3);display:inline-flex;align-items:center;gap:var(--sp-3);}
.dvc-gauge .g-lab svg{width:var(--sp-6);height:var(--sp-6);color:var(--ink-4);}
.dvc-gauge .g-val{font-family:var(--font-disp);font-size:var(--fs-xl);font-weight:var(--fw-medium);color:var(--ink);letter-spacing:-.02em;margin-left:auto;}
.dvc-gauge .g-val small{font-size:var(--fs-meta);color:var(--ink-3);font-weight:var(--fw-medium);margin-left:1px;}
.dvc-gauge .g-sub{font-size:var(--fs-meta);color:var(--ink-4);}
.dvc-kind{display:inline-grid;place-items:center;width:17px;height:17px;border-radius:4px;flex:none;}
.dvc-kind svg{width:var(--sp-5);height:var(--sp-5);}
.dvc-split{flex:none;display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex-wrap:nowrap;min-width:0;}
.dvc-split>span{white-space:nowrap;}
/* the verbose right-side note yields (truncates) before the numbers ever wrap */
.dvc-split>span:last-child{min-width:0;overflow:hidden;text-overflow:ellipsis;}
.dvc-palert{flex:none;display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-6);color:var(--ink);background:color-mix(in oklch,var(--st-blocked),transparent 91%);border-bottom:1px solid color-mix(in oklch,var(--st-blocked),transparent 62%);}
.dvc-palert b{font-weight:var(--fw-medium);}
.dvc-twbtn .kj-button{border:none;background:transparent;padding:0;height:auto;cursor:pointer;display:inline-flex;color:var(--ink-4);flex:none;}
.dvc-twbtn .kj-button:hover{color:var(--ink-2);}
.dvc-twbtn .dvc-tw.open{transform:rotate(90deg);color:var(--ui-ink);}
.dvc-pdot{width:7px;height:7px;border-radius:2px;flex:none;}
.dvc-recdot{width:var(--sp-3);height:var(--sp-3);border-radius:50%;background:var(--lat-r);animation:dvcblink 1.5s ease-in-out infinite;}
.dvc-foot{flex:none;display:flex;align-items:center;gap:var(--sp-6);padding:var(--sp-4) var(--sp-6);border-top:1px solid var(--hair);background:var(--panel-2);color:var(--ink-3);}
.dvc-leg{display:flex;align-items:center;gap:var(--sp-3);}
.dvc-sw{width:var(--sp-4);height:var(--sp-4);border-radius:3px;}
.dvc-sw.g{background:var(--lat-g);} .dvc-sw.a{background:var(--lat-a);} .dvc-sw.r{background:var(--lat-r);}
`,
  ],
})
export class DevPanelComponent {
  readonly perf = inject(PerfStore);
  /** Live terminal write-scheduler counters (PTY pipeline visibility). */
  readonly termStats = terminalSchedulerStats;
  /** Batched-PTY throughput line: KB/s · emit rate · avg batch size. Volume is
   *  what batching optimizes, so rate alone (calls/10s) undersells the row. */
  readonly ptyLine = computed(() => {
    const r = this.perf.rows().find((x) => x.cmd === "agent_output_emit");
    if (!r?.bytes10s || !r.calls10s) return null;
    const kbs = r.bytes10s / 10 / 1024;
    const rate = Math.round(r.calls10s / 10);
    const batch = Math.round(r.bytes10s / r.calls10s);
    return `pty ${kbs >= 100 ? Math.round(kbs) : kbs.toFixed(1)} KB/s · ${rate} ev/s · ${batch} B/batch`;
  });
  private readonly runtime = inject(AgentRuntimeService);
  private readonly projectsStore = inject(ProjectsStore);
  private readonly metricsStore = inject(MetricsStore);
  private readonly panel = inject(DevPanelStore);
  private readonly bridge = inject(BRIDGE);
  /** Raw emit-trace indicator (A0.7) — the Emits tab's record chip + toggle. */
  readonly telemetry = inject(TelemetryStore);

  /** Build tier — metadata for perf exports only; the panel shows everything in prod too. */
  readonly dev = isDevMode();
  /** Panel visibility + active tab live in DevPanelStore so other surfaces
   *  (status-bar cpu/mem readout) can deep-link; a constructor effect mirrors
   *  visibility into the scheduler stats gate. */
  readonly open = this.panel.open;
  readonly tab = this.panel.tab;
  readonly sort = signal<Sort>({ key: "rt", dir: -1 });
  readonly aSort = signal<Sort>({ key: "status", dir: 1 });
  readonly pSort = signal<Sort>({ key: "name", dir: 1 });
  readonly openCmd = signal<string | null>(null);
  readonly openAg = signal<string | null>(null);
  readonly openPr = signal<string | null>(null);
  readonly feedOpen = signal(false);
  /** Transient "Copied" state for the Copy button. */
  readonly copied = signal(false);

  readonly agents = computed(() => this.runtime.agents());
  readonly projects = computed(() => this.projectsStore.all());

  readonly PCOLS: [string, string][] = [["cmd", "command"], ["calls", "calls/10s"], ["rt", "avg RT"], ["exec", "avg exec"], ["overhead", "overhead"], ["p95", "p95"], ["max", "max"], ["err", "err%"], ["spark", "trend"]];
  readonly ACOLS: [string, string][] = [["name", "agent"], ["status", "status"], ["tool", "tool"], ["project", "project"], ["branch", "branch"], ["commits", "c"], ["elapsed", "elapsed"], ["progress", "prog"]];
  readonly PRCOLS: [string, string][] = [["name", "project"], ["id", "id"], ["head", "head"], ["branch", "default"], ["branches", "branches"], ["agents", "agents"], ["files", "files"]];

  private tickIv?: ReturnType<typeof setInterval>;
  private copiedTo?: ReturnType<typeof setTimeout>;

  constructor() {
    // close on Escape / click outside the FAB + panel (both live under this
    // component's host, which carries the shared DismissDirective)
    inject(DismissDirective).appDismiss.subscribe(() => {
      if (this.open()) this.open.set(false);
    });
    // gate the terminal write-scheduler stats collector on panel visibility
    effect(() => setSchedulerStatsEnabled(this.open()));
    // age out the 10s window while the panel is open; the same 1s heartbeat
    // drives the pull-based Resources/Emits polls (every 2nd tick — matching
    // the perf://stats push cadence) so the samplers cost NOTHING while the
    // panel is closed or another tab is showing (the A1.7 gating pattern).
    this.tickIv = setInterval(() => {
      if (!this.open()) return;
      if (this.tab() === "perf") this.perf.tick();
      this.pollTick++;
      if (this.pollTick % 2 === 0) {
        if (this.tab() === "resources") void this.pollTree();
        if (this.tab() === "emits") void this.pollEmits();
      }
    }, 1000);
    // first paint on tab reveal — don't wait out the poll interval; the tree
    // reveal also forces a discovery sweep (children spawned since the last
    // sweep are otherwise invisible for up to 10 scoped polls)
    effect(() => {
      if (!this.open()) return;
      const t = this.tab();
      if (t === "resources") void this.pollTree(true);
      if (t === "emits") void this.pollEmits();
    });
    inject(DestroyRef).onDestroy(() => {
      clearInterval(this.tickIv);
      clearTimeout(this.copiedTo);
      setSchedulerStatsEnabled(false);
    });
  }

  // ── perf ──
  // table persists once any command has been seen (don't blank out when idle)
  readonly hasCalls = computed(() => this.perf.rows().length > 0);
  readonly sortedPerf = computed<PerfRow[]>(() => {
    const rows = this.perf.rows().slice();
    const { key, dir } = this.sort();
    const get = (r: PerfRow): number =>
      key === "calls" ? r.calls10s : key === "rt" ? (r.avgRt ?? -1) : key === "exec" ? (r.avgExec ?? -1) : key === "overhead" ? (r.overhead ?? -1) : key === "p95" ? (r.p95Rt ?? -1) : key === "max" ? (r.maxRt ?? -1) : key === "err" ? r.errPct : -1;
    rows.sort((a, b) => {
      if (key === "cmd" || key === "spark") return a.cmd < b.cmd ? -dir : a.cmd > b.cmd ? dir : 0;
      return (get(a) - get(b)) * dir;
    });
    return rows;
  });
  readonly feed = computed(() => {
    const all = this.perf.rows().flatMap((r) => r.recent.map((s) => ({ ts: s.ts, cmd: r.cmd, ms: s.ms, ok: s.ok })));
    all.sort((a, b) => b.ts - a.ts);
    return all.slice(0, 30);
  });
  readonly perfSummary = computed(() => {
    const rows = this.perf.rows();
    if (!rows.length) return "0 commands · 0 calls";
    const calls = rows.reduce((a, r) => a + r.calls10s, 0);
    const slow = rows.filter((r) => r.avgRt != null && r.avgRt > 100).length;
    return `${rows.length} commands · ${calls} calls/10s · ${slow} >100ms`;
  });
  /** Commands currently breaching budget (>100ms avg RT or erroring) — the FAB badge. */
  readonly alertCount = computed(() => this.perf.rows().filter((r) => !r.stale && (r.errPct > 0 || (r.avgRt != null && r.avgRt > 100))).length);
  clickPerfSort(k: string) {
    this.sort.update((s) => (s.key === k ? { key: k, dir: -s.dir } : { key: k, dir: k === "cmd" ? 1 : -1 }));
  }

  /** Copy the perf aggregates as JSON so they can be pasted back for bottleneck
   *  analysis. Rows follow the visible (sorted) order — slowest first by default.
   *  Aggregates only: drops per-command `hist`/`recent`. Floats rounded to 1dp. */
  copyPerf(): void {
    const r1 = (v: number | null) => (v == null ? null : Math.round(v * 10) / 10);
    const envelope = {
      capturedAt: new Date().toISOString(),
      tier: this.dev ? "dev" : "prod",
      windowMs: PERF_WINDOW_MS,
      rows: this.sortedPerf().map((r) => ({
        cmd: r.cmd,
        calls10s: r.calls10s,
        avgRt: r1(r.avgRt),
        avgExec: r1(r.avgExec),
        overhead: r1(r.overhead),
        p95Rt: r1(r.p95Rt),
        maxRt: r1(r.maxRt),
        errPct: r1(r.errPct),
      })),
    };
    void navigator.clipboard
      .writeText(JSON.stringify(envelope, null, 2))
      .then(() => {
        this.copied.set(true);
        clearTimeout(this.copiedTo);
        this.copiedTo = setTimeout(() => this.copied.set(false), 1200);
      })
      .catch(() => {});
  }

  ms(v: number | null): string {
    return v == null ? "—" : v >= 100 ? Math.round(v) + "ms" : v >= 10 ? v.toFixed(0) + "ms" : v.toFixed(1) + "ms";
  }
  ovh(v: number | null): string {
    return v == null ? "—" : (v < 10 ? v.toFixed(1) : Math.round(v)) + "ms";
  }
  lat(v: number | null): string {
    return v == null ? "na" : v <= 16 ? "g" : v <= 100 ? "a" : "r";
  }
  latColor(c: string): string {
    return c === "r" ? "var(--lat-r)" : c === "a" ? "var(--lat-a)" : "var(--lat-g)";
  }
  sparkPoints(data: number[], w = 58): string {
    if (!data.length) return "";
    const h = 16, max = Math.max(...data, 1);
    const step = data.length > 1 ? w / (data.length - 1) : w;
    return data.map((v, i) => `${(i * step).toFixed(1)},${(h - (v / max) * (h - 2) - 1).toFixed(2)}`).join(" ");
  }
  clock(ts: number): string {
    const d = new Date(ts), p = (n: number, l = 2) => String(n).padStart(l, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  }

  // ── agents ──
  private readonly SPRIO: Record<string, number> = { blocked: 0, running: 1, waiting: 2, queued: 3, done: 4, idle: 5 };
  readonly sortedAgents = computed<Agent[]>(() => {
    const arr = this.agents().slice();
    const { key, dir } = this.aSort();
    arr.sort((a, b) => {
      if (key === "status") return (this.SPRIO[a.status] - this.SPRIO[b.status]) * dir;
      if (key === "project") return a.projectId < b.projectId ? -dir : dir;
      // live elapsed comes from the runtime clock, not the Agent record
      if (key === "elapsed") return (this.elapsed(a) - this.elapsed(b)) * dir;
      const av = (a as unknown as Record<string, unknown>)[key];
      const bv = (b as unknown as Record<string, unknown>)[key];
      if (typeof av === "number" && typeof bv === "number") return (av - bv) * dir;
      return String(av) < String(bv) ? -dir : dir;
    });
    return arr;
  });
  clickASort(k: string) {
    this.aSort.update((s) => (s.key === k ? { key: k, dir: -s.dir } : { key: k, dir: 1 }));
  }
  elapsed(ag: Agent): number {
    return this.runtime.elapsedFor(ag.id);
  }
  attn(ag: Agent): boolean {
    return ag.status === "blocked" || !!ag.needsInput || (ag.pending ?? []).some((p) => p.kind === "permission" || p.kind === "decision");
  }
  note(ag: Agent): string | null {
    if (ag.blockReason) return ag.blockReason;
    if (ag.waitReason) return ag.waitReason;
    const r = (ag.pending ?? []).find((p) => p.kind === "review" || p.kind === "decision");
    return r ? r.title : null;
  }
  cnt(st: AgentStatus): number {
    return this.agents().filter((a) => a.status === st).length;
  }
  statusColor(s: AgentStatus): string {
    const m: Record<string, string> = { running: "var(--st-running)", blocked: "var(--st-blocked)", done: "var(--st-done)", waiting: "var(--st-waiting)", queued: "var(--ink-3)", idle: "var(--ink-4)" };
    return m[s] ?? "var(--ink-3)";
  }
  fmtDur(sec: number): string {
    if (!sec) return "0s";
    if (sec < 60) return sec + "s";
    const m = Math.floor(sec / 60), s = sec % 60;
    if (m < 60) return m + "m" + (s ? " " + s + "s" : "");
    return Math.floor(m / 60) + "h " + (m % 60) + "m";
  }
  pct(v: number): number {
    return Math.round(v * 100);
  }

  // ── projects ──
  readonly sortedProjects = computed<Project[]>(() => {
    const arr = this.projects().slice();
    const { key, dir } = this.pSort();
    arr.sort((a, b) => {
      if (key === "branches") return ((a.branches ?? []).length - (b.branches ?? []).length) * dir;
      if (key === "agents") return (this.agentsIn(a.id).length - this.agentsIn(b.id).length) * dir;
      if (key === "files") return ((a.files ?? []).length - (b.files ?? []).length) * dir;
      const av = (a as unknown as Record<string, unknown>)[key];
      const bv = (b as unknown as Record<string, unknown>)[key];
      return String(av) < String(bv) ? -dir : dir;
    });
    return arr;
  });
  clickPSort(k: string) {
    this.pSort.update((s) => (s.key === k ? { key: k, dir: -s.dir } : { key: k, dir: 1 }));
  }
  proj(id: string): Project {
    return this.projects().find((p) => p.id === id) ?? ({ name: id, color: "var(--ink-3)", icon: "box" } as Project);
  }
  agentsIn(id: string): Agent[] {
    return this.agents().filter((a) => a.projectId === id);
  }
  needsIn(id: string): number {
    return this.agentsIn(id).filter((a) => this.attn(a)).length;
  }

  // ── resources (gauges from the system://metrics push — always warm) ──
  readonly procs = computed(() => this.metricsStore.metrics()?.procs ?? []);
  readonly totalCpu = computed(() => this.metricsStore.metrics()?.totalCpu ?? 0);
  readonly cores = computed(() => this.metricsStore.metrics()?.cores ?? 1);
  readonly coresUsed = computed(() => ((this.totalCpu() / 100) * this.cores()).toFixed(2));
  readonly memGb = computed(() => ((this.metricsStore.metrics()?.totalMemBytes ?? 0) / 2 ** 30).toFixed(2));
  readonly sysGb = computed(() => Math.round((this.metricsStore.metrics()?.sysMemBytes ?? 0) / 2 ** 30));
  readonly memPct = computed(() => {
    const m = this.metricsStore.metrics();
    return m?.sysMemBytes ? (m.totalMemBytes / m.sysMemBytes) * 100 : 0;
  });
  readonly agentProcCount = computed(() => this.procs().filter((p) => p.id !== "app").length);
  readonly gaugeCpuC = computed(() => (this.totalCpu() < 30 ? "g" : this.totalCpu() < 70 ? "a" : "r"));
  readonly gaugeMemC = computed(() => (this.memPct() < 25 ? "g" : this.memPct() < 50 ? "a" : "r"));
  barClamp(v: number): number {
    return Math.min(100, Math.max(2, v));
  }
  agentOf(id: string): Agent | undefined {
    return id === "app" ? undefined : this.agents().find((a) => a.id === id);
  }

  // ── resources tree (A7.7 recursive, merged under one "Orrery App" root) ──
  private pollTick = 0;
  private treeBusy = false;
  readonly tree = signal<ProcessTreeSnapshot | null>(null);
  /** Per-node expand state (`rootId:pid` → open). Absent = open, except the
   *  webview family which pollTree seeds collapsed ("not our code" is noise). */
  private readonly treeExpand = signal<Record<string, boolean>>({});

  /** `discover` forces a full backend sweep — passed on tab reveal so children
   *  spawned since the last sweep (the WebView2 family right after startup)
   *  show immediately instead of after the slow rediscovery cadence. */
  private async pollTree(discover = false): Promise<void> {
    if (this.treeBusy) return; // one in-flight snapshot at a time
    this.treeBusy = true;
    try {
      const t = await this.bridge.invoke<ProcessTreeSnapshot>(Commands.ProcessTree, { discover });
      // seed webview-family nodes collapsed (only when unseen — user toggles win)
      const seed: Record<string, boolean> = {};
      const walk = (n: ProcessNode, rootId: string) => {
        // Only EXCLUDED (external-browser) nodes seed collapsed. The WebView2
        // family is Orrery's own UI runtime — shown expanded like everything
        // else we spawn (the user's model: Orrery == the whole Tauri runtime).
        if (n.children.length && n.excluded) seed[rootId + ":" + n.pid] = false;
        for (const c of n.children) walk(c, rootId);
      };
      for (const r of t.roots) walk(r.node, r.id);
      this.treeExpand.update((prev) => ({ ...seed, ...prev }));
      this.tree.set(t);
    } catch {
      // backend unavailable — tab keeps its empty state
    } finally {
      this.treeBusy = false;
    }
  }

  expanded(key: string): boolean {
    return this.treeExpand()[key] ?? true;
  }
  toggleNode(key: string): void {
    const open = this.expanded(key);
    this.treeExpand.update((m) => ({ ...m, [key]: !open }));
  }

  /** One synthetic "Orrery App" root over the backend's forest — always the
   *  first row, always the recursive total. Null until the first sample. */
  readonly mergedRoot = computed(() => {
    const t = this.tree();
    return t ? mergeRoots(t) : null;
  });
  readonly treeRows = computed(() => {
    const t = this.tree();
    const root = this.mergedRoot();
    if (!t || !root) return [];
    const ex = this.treeExpand();
    const rows: { key: string; n: ProcessNode; depth: number; rootId: string; agentRoot: boolean }[] = [];
    const walk = (n: ProcessNode, depth: number, rootId: string, agentRoot: boolean) => {
      const key = rootId + ":" + n.pid;
      rows.push({ key, n, depth, rootId, agentRoot });
      if ((ex[key] ?? true) && n.children.length) for (const c of n.children) walk(c, depth + 1, rootId, false);
    };
    // Absent expand state = open, so the root ships expanded by default.
    rows.push({ key: MERGED_ROOT_KEY, n: root, depth: 0, rootId: "app", agentRoot: false });
    if (ex[MERGED_ROOT_KEY] ?? true) {
      // Walk the ORIGINAL roots (same nodes as root.children) so expand keys
      // stay `rootId:pid` — stable across polls and across the old tab's seeds.
      for (const r of t.roots) walk(r.node, 1, r.id, r.id !== "app");
    }
    return rows;
  });
  readonly treeProcCount = computed(() =>
    (this.tree()?.roots ?? []).reduce((a, r) => a + r.node.subtreeProcs, 0),
  );
  /** The status-bar split's drill-down numbers: Orrery = the app root's
   *  subtree; agents = every agent root summed. Private bytes, never RSS. */
  readonly orreryPriv = computed(
    () => this.tree()?.roots.find((r) => r.id === "app")?.node.subtreePrivBytes ?? 0,
  );
  readonly agentsPriv = computed(() =>
    (this.tree()?.roots ?? [])
      .filter((r) => r.id !== "app")
      .reduce((a, r) => a + r.node.subtreePrivBytes, 0),
  );
  /** Why 700MB: the runaway process is usually the dev server / test runner an
   *  agent started and never cleaned up — flag any agent subtree above it. */
  private readonly ALERT_PRIV_BYTES = 700 * 1024 * 1024;
  readonly treeAlerts = computed(() =>
    (this.tree()?.roots ?? [])
      .filter((r) => r.id !== "app" && r.node.subtreePrivBytes > this.ALERT_PRIV_BYTES)
      .map((r) => ({
        id: r.id,
        label: r.label,
        priv: r.node.subtreePrivBytes,
        childPriv: r.node.subtreePrivBytes - r.node.privBytes,
        childNames: r.node.children.map((c) => c.name).join(", ") || "its descendants",
      })),
  );
  readonly treeAge = computed(() => {
    const ts = this.tree()?.tsMs ?? 0;
    if (!ts) return "—";
    const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
    return s <= 2 ? "just now" : `${s}s ago`;
  });
  notOurs(n: ProcessNode): boolean {
    return !!n.note?.includes("not our code");
  }
  nodeDot(n: ProcessNode): string {
    if (this.notOurs(n)) return "var(--ink-4)";
    if (n.detached) return "var(--lat-a)";
    if (n.note?.includes("shim wrapper")) return "var(--lat-r)";
    return "var(--ui-fill)";
  }

  // ── emits (A0.7 phase 1 aggregate) ──
  private readonly emitRowsSig = signal<EmitAggRow[]>([]);
  private readonly traceState = signal<TelemetryTraceState | null>(null);
  readonly eSort = signal<Sort>({ key: "bytes", dir: -1 });
  readonly ECOLS: [string, string][] = [["name", "event name"], ["count", "calls"], ["bytes", "total bytes"], ["p50", "p50"], ["p95", "p95"], ["rate", "peak/s"], ["class", "suggested class"]];

  private async pollEmits(): Promise<void> {
    try {
      this.emitRowsSig.set(await this.bridge.invoke<EmitAggRow[]>(Commands.TelemetryEmits));
      this.traceState.set(
        await this.bridge.invoke<TelemetryTraceState>(Commands.TelemetryTraceState),
      );
    } catch {
      // backend unavailable — tab keeps its empty state
    }
  }

  readonly emitRows = computed(() => this.emitRowsSig());
  readonly emitTotal = computed(() => this.emitRowsSig().reduce((a, r) => a + r.totalBytes, 0));
  readonly sortedEmits = computed<EmitAggRow[]>(() => {
    const rows = this.emitRowsSig().slice();
    const { key, dir } = this.eSort();
    const get = (r: EmitAggRow): number =>
      key === "count" ? r.count : key === "bytes" ? r.totalBytes : key === "p50" ? r.p50Bytes : key === "p95" ? r.p95Bytes : key === "rate" ? r.peakPerSec : -1;
    rows.sort((a, b) => {
      if (key === "name") return a.name < b.name ? -dir : a.name > b.name ? dir : 0;
      if (key === "class") return this.suggestClass(a) < this.suggestClass(b) ? -dir : dir;
      return (get(a) - get(b)) * dir;
    });
    return rows;
  });
  clickESort(k: string) {
    this.eSort.update((s) => (s.key === k ? { key: k, dir: -s.dir } : { key: k, dir: k === "name" || k === "class" ? 1 : -1 }));
  }
  /** Heuristic seed for the Phase-2 class taxonomy (roadmap decision 8) —
   *  mirrors scripts/telemetry/summarize.mjs. NOT the decision. */
  suggestClass(r: EmitAggRow): string {
    if (r.p95Bytes <= 512 && r.peakPerSec <= 2) return "bypass";
    if (r.p95Bytes <= 4096 && r.peakPerSec <= 30) return "coalesced state";
    return "bulk weighted";
  }
  clsColor(r: EmitAggRow): string {
    const c = this.suggestClass(r);
    return c === "bypass" ? "var(--lat-g)" : c === "coalesced state" ? "var(--ink-2)" : "var(--ui-ink)";
  }
  /** " · 12m / 30m · 3.1 MB" detail after the recording chip. */
  traceLine(): string {
    const s = this.traceState();
    if (!s?.active) return "";
    const mins = Math.floor((Date.now() - s.startedMs) / 60_000);
    return ` · ${mins}m / 30m · ${this.fmtBytes(s.bytes)}`;
  }
  toggleTrace(): void {
    this.telemetry.setTrace(!this.telemetry.traceActive());
    // reflect the new state promptly in the detail line
    void this.pollEmits();
  }
  /** Compact byte formatting for the emit table (B / KB / MB / GB). */
  fmtBytes(n: number): string {
    if (n >= 2 ** 30) return (n / 2 ** 30).toFixed(2) + " GB";
    if (n >= 2 ** 20) return (n / 2 ** 20).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }
  /** Human-readable bytes: B / KB / MB / GB, one decimal above KB. */
  fmtMem(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const kb = bytes / 1024;
    if (kb < 1024) return `${Math.round(kb)} KB`;
    const mb = kb / 1024;
    if (mb < 1024) return `${mb.toFixed(1)} MB`;
    return `${(mb / 1024).toFixed(1)} GB`;
  }
}
