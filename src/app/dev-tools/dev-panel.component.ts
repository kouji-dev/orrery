import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  HostListener,
  OnDestroy,
  ViewEncapsulation,
  computed,
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
import { Agent, AgentStatus, Project } from "../models";

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
  imports: [IconComponent, StatusDotComponent, ToolBadgeComponent],
  template: `
    <button class="dvc-fab" [class.on]="open()" title="Dev console" aria-label="Dev console" (click)="open.set(!open())">
      @if (!open()) { <span class="dvc-pulse"></span> }
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12h3l2.5-6 4 13 3-9 1.5 2H21" /></svg>
    </button>

    @if (open()) {
      <section class="dvcon" aria-label="Dev console">
        <header class="dvc-head">
          <span class="dvc-brand"><app-icon name="cpu" size="sm" /></span>
          <div class="dvc-tabs">
            <button class="dvc-tab" [class.on]="tab() === 'perf'" (click)="tab.set('perf')"><app-icon name="spark" size="sm" />Perf</button>
            <button class="dvc-tab" [class.on]="tab() === 'agents'" (click)="tab.set('agents')"><app-icon name="agent" size="sm" />Agents<span class="dvc-cnt">{{ agents().length }}</span></button>
            <button class="dvc-tab" [class.on]="tab() === 'projects'" (click)="tab.set('projects')"><app-icon name="box" size="sm" />Projects<span class="dvc-cnt">{{ projects().length }}</span></button>
          </div>
          <span class="dvc-live on"><span class="dvc-ld"></span>live</span>
          <span class="dvc-sp"></span>
          @if (tab() === 'perf') {
            @if (hasCalls()) {
              <button class="dvc-ic" [class.ok]="copied()" (click)="copyPerf()" [title]="copied() ? 'Copied to clipboard' : 'Copy perf data as JSON'"><app-icon [name]="copied() ? 'check' : 'dup'" size="sm" />{{ copied() ? 'Copied' : 'Copy' }}</button>
            }
            <button class="dvc-ic" (click)="perf.clear()" title="Clear counters"><app-icon name="refresh" size="sm" />Reset</button>
          }
          <button class="dvc-x" (click)="open.set(false)" title="Close"><app-icon name="x" size="sm" /></button>
        </header>

        <div class="dvc-body">
          <!-- ── PERF ── -->
          @if (tab() === 'perf') {
            @if (hasCalls()) {
              <div class="dvc-scroll">
              <table class="dvc-tbl">
                <thead><tr>
                  @for (c of PCOLS; track c[0]) {
                    <th [class.srt]="sort().key === c[0]" (click)="clickPerfSort(c[0])">{{ c[1] }}@if (sort().key === c[0]) { <span class="dvc-arr">{{ sort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                  }
                </tr></thead>
                <tbody>
                  @for (s of sortedPerf(); track s.cmd) {
                    <tr class="dvc-row" [class.open]="openCmd() === s.cmd" (click)="dev && openCmd.set(openCmd() === s.cmd ? null : s.cmd)">
                      <td><span class="dvc-lead">@if (dev) { <svg class="dvc-tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg> }<span class="dvc-nm">{{ s.cmd }}</span></span></td>
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
                    @if (openCmd() === s.cmd && dev) {
                      <tr class="dvc-detail"><td [attr.colspan]="PCOLS.length"><div class="dvc-din">
                        <div class="dvc-dh"><span class="lbl">recent · {{ s.cmd }}</span><span class="dvc-ds tnum"><span>p95 <b>{{ ms(s.p95Rt) }}</b></span><span>max <b>{{ ms(s.maxRt) }}</b></span><span>overhead <b>{{ ovh(s.overhead) }}</b></span></span></div>
                        <div class="dvc-calls">
                          @for (c of s.recent; track $index) {
                            <div class="dvc-cr"><span class="ts tnum">{{ clock(c.ts) }}</span><span class="cm">{{ s.cmd }}</span><span [class]="'du ' + lat(c.ms) + ' tnum'">{{ ms(c.ms) }}</span><span [class]="'dvc-dot ' + (c.ok ? 'ok' : 'er')"></span></div>
                          }
                        </div>
                      </div></td></tr>
                    }
                  }
                </tbody>
              </table>
              </div>
            } @else {
              <div class="dvc-empty">
                <div class="dvc-ring"><app-icon name="spark" /></div>
                <h4>No calls yet</h4>
                <p>Invoke metrics populate as the frontend dispatches Tauri commands. Counters reset on a rolling 10-second window.</p>
                <span class="dvc-hint"><span class="dvc-ld"></span>listening for invokes…</span>
              </div>
            }
            @if (hasCalls() && dev) {
              <div class="dvc-feed">
                <button class="dvc-fh" [class.open]="feedOpen()" (click)="feedOpen.set(!feedOpen())">
                  <svg class="dvc-tw" [class.open]="feedOpen()" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>
                  <span class="lbl">Recent calls</span>
                  <app-icon name="timeline" size="sm" [color]="'var(--ink-4)'" />
                  <span class="ct">last {{ feed().length }}</span>
                </button>
                @if (feedOpen()) {
                  <div class="dvc-fl">
                    @for (f of feed(); track f.ts + f.cmd) {
                      <div class="dvc-fr"><span class="ts tnum">{{ clock(f.ts) }}</span><span [class]="'dvc-dot ' + (f.ok ? 'ok' : 'er')" style="justify-self:start"></span><span class="cm">{{ f.cmd }}</span><span [class]="'du ' + lat(f.ms) + ' tnum'">{{ ms(f.ms) }}</span><span class="ix tnum">{{ f.ok ? 'ok' : 'err' }}</span></div>
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
                  <th [class.srt]="aSort().key === c[0]" (click)="clickASort(c[0])">{{ c[1] }}@if (aSort().key === c[0]) { <span class="dvc-arr">{{ aSort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                }
              </tr></thead>
              <tbody>
                @for (ag of sortedAgents(); track ag.id) {
                  <tr class="dvc-row" [class.open]="openAg() === ag.id" (click)="openAg.set(openAg() === ag.id ? null : ag.id)">
                    <td><span class="dvc-lead"><svg class="dvc-tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg><span class="dvc-id">{{ ag.id.slice(0, 8) }}</span><span class="dvc-nm">{{ ag.name }}</span>@if (attn(ag)) { <span class="dvc-needs" title="needs you"></span> }</span></td>
                    <td><span class="dvc-stat"><span class="lb" [style.color]="statusColor(ag.status)">{{ ag.status }}</span><app-status-dot [status]="ag.status" /></span></td>
                    <td><span class="dvc-tool" style="vertical-align:middle" [title]="ag.tool + ' · ' + ag.model"><app-tool-badge [tool]="ag.tool" [size]="17" /></span></td>
                    <td><span class="dvc-pchip"><span class="dvc-pn">{{ proj(ag.projectId).name }}</span><span class="dvc-pi" [style.background]="'color-mix(in oklch,' + proj(ag.projectId).color + ',transparent 82%)'"><app-icon [name]="proj(ag.projectId).icon" size="sm" [px]="11" [color]="proj(ag.projectId).color" /></span></span></td>
                    <td><span class="dvc-branch">{{ ag.branch.replace('agent/', '') }}</span></td>
                    <td class="tnum">{{ ag.commits }}</td>
                    <td class="tnum" style="color:var(--ink-3)">{{ ag.elapsed ? fmtDur(ag.elapsed) : '—' }}</td>
                    <td class="tnum"><span class="dvc-mtr"><i [style.width.%]="pct(ag.progress)" [style.background]="statusColor(ag.status)"></i></span>{{ pct(ag.progress) }}%</td>
                  </tr>
                  @if (openAg() === ag.id) {
                    <tr class="dvc-detail"><td [attr.colspan]="ACOLS.length"><div class="dvc-din">
                      <div class="dvc-task">{{ ag.task }}</div>
                      @if (attn(ag) && note(ag)) { <div class="dvc-attn"><app-icon name="flag" size="sm" />{{ note(ag) }}</div> }
                      <div class="dvc-kv">
                        <div class="r"><span class="k">id</span><span class="vv"><b>{{ ag.id }}</b></span></div>
                        <div class="r"><span class="k">tool</span><span class="vv">{{ ag.tool }} · {{ ag.model }}{{ ag.effort ? ' · ' + ag.effort : '' }}</span></div>
                        <div class="r"><span class="k">status</span><span class="vv">{{ ag.status }}{{ ag.pending.length ? ' · ' + ag.pending.length + ' pending' : '' }}</span></div>
                        <div class="r"><span class="k">branch</span><span class="vv">{{ ag.branch }}</span></div>
                        <div class="r"><span class="k">worktree</span><span class="vv">{{ ag.worktree }}</span></div>
                        <div class="r"><span class="k">base</span><span class="vv">{{ ag.base }}</span></div>
                        <div class="r"><span class="k">commits</span><span class="vv">{{ ag.commits }}</span></div>
                        <div class="r"><span class="k">progress</span><span class="vv">{{ pct(ag.progress) }}%</span></div>
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
                  <th [class.srt]="pSort().key === c[0]" (click)="clickPSort(c[0])">{{ c[1] }}@if (pSort().key === c[0]) { <span class="dvc-arr">{{ pSort().dir < 0 ? '▼' : '▲' }}</span> }</th>
                }
              </tr></thead>
              <tbody>
                @for (p of sortedProjects(); track p.id) {
                  <tr class="dvc-row" [class.open]="openPr() === p.id" (click)="openPr.set(openPr() === p.id ? null : p.id)">
                    <td><span class="dvc-lead"><svg class="dvc-tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg><span class="dvc-pi" style="width:18px;height:18px;border-radius:5px" [style.background]="'color-mix(in oklch,' + p.color + ',transparent 82%)'"><app-icon [name]="p.icon" size="sm" [px]="12" [color]="p.color" /></span><span class="dvc-nm" [style.color]="p.color">{{ p.name }}</span></span></td>
                    <td class="dvc-id tnum">{{ p.id.slice(0, 8) }}</td>
                    <td class="tnum" style="color:var(--ink-3)">{{ p.head ?? '—' }}</td>
                    <td style="color:var(--ink-3)">{{ p.branch ?? '—' }}</td>
                    <td class="tnum">{{ (p.branches ?? []).length }}</td>
                    <td class="tnum">{{ agentsIn(p.id).length }}@if (needsIn(p.id) > 0) { <span style="color:var(--st-blocked);font-weight:700"> {{ needsIn(p.id) }}!</span> }</td>
                    <td class="tnum">{{ (p.files ?? []).length }}</td>
                  </tr>
                  @if (openPr() === p.id) {
                    <tr class="dvc-detail"><td [attr.colspan]="PRCOLS.length"><div class="dvc-din">
                      <div class="dvc-kv">
                        <div class="r"><span class="k">id</span><span class="vv"><b>{{ p.id }}</b></span></div>
                        <div class="r"><span class="k">path</span><span class="vv">{{ p.path }}</span></div>
                        <div class="r"><span class="k">repo</span><span class="vv">{{ p.repo ?? '—' }}</span></div>
                        <div class="r"><span class="k">head</span><span class="vv">{{ p.head ?? '—' }} ({{ p.branch ?? '—' }})</span></div>
                        <div class="r"><span class="k">hasGit</span><span class="vv">{{ p.hasGit }}</span></div>
                        <div class="r"><span class="k">folderExists</span><span class="vv">{{ p.folderExists }}</span></div>
                      </div>
                      @if ((p.branches ?? []).length) {
                        <div><div class="dvc-sl">branches · {{ (p.branches ?? []).length }}</div><div style="display:flex;flex-wrap:wrap;gap:6px">@for (b of p.branches ?? []; track b) { <span class="dvc-chip"><app-icon name="branch" size="sm" [px]="11" />{{ b }}</span> }</div></div>
                      }
                      <div><div class="dvc-sl">agents · {{ agentsIn(p.id).length }}</div><div class="dvc-files">@for (a of agentsIn(p.id); track a.id) { <div class="dvc-frow" style="grid-template-columns:9px 1fr auto"><app-status-dot [status]="a.status" /><span class="dvc-fp">{{ a.id.slice(0, 8) }} · {{ a.name }}</span><span class="dvc-chip"><app-tool-badge [tool]="a.tool" [size]="13" />{{ a.status }}</span></div>} @empty { <div class="dvc-dim" style="font-size:11px">no agents</div> }</div></div>
                    </div></td></tr>
                  }
                }
              </tbody>
            </table>
            </div>
          }
        </div>

        <footer class="dvc-foot">
          @if (tab() === 'perf') {
            <span class="dvc-leg"><span class="dvc-sw g"></span>≤16ms</span>
            <span class="dvc-leg"><span class="dvc-sw a"></span>16–100ms</span>
            <span class="dvc-leg"><span class="dvc-sw r"></span>&gt;100ms · err</span>
            <span class="dvc-sp"></span>
            <span class="tnum">{{ perfSummary() }}</span>
          }
          @if (tab() === 'agents') {
            <span class="tnum">{{ agents().length }} agents · <span style="color:var(--st-running)">{{ cnt('running') }} running</span> · <span style="color:var(--st-blocked)">{{ cnt('blocked') }} need you</span> · {{ cnt('waiting') + cnt('queued') }} waiting · {{ cnt('done') }} done</span>
            <span class="dvc-sp"></span><span class="tnum">live state</span>
          }
          @if (tab() === 'projects') {
            <span class="tnum">{{ projects().length }} projects · {{ agents().length }} worktrees</span>
            <span class="dvc-sp"></span><span class="tnum">live state</span>
          }
        </footer>
      </section>
    }
  `,
  styles: [
    `
.dvcon{--lat-g:#35e0a1;--lat-a:#f5c451;--lat-r:#ff5d7a;
  --lat-g-bg:rgba(53,224,161,.08);--lat-a-bg:rgba(245,196,81,.13);--lat-r-bg:rgba(255,93,122,.18);
  --lat-r-ring:rgba(255,93,122,.4);}
[data-theme="light"] .dvcon{--lat-g:#0a8f5e;--lat-a:#a9700f;--lat-r:#d6304e;
  --lat-g-bg:rgba(10,143,94,.09);--lat-a-bg:rgba(169,112,15,.13);--lat-r-bg:rgba(214,48,78,.14);
  --lat-r-ring:rgba(214,48,78,.34);}
.dvc-fab{position:relative;width:44px;height:44px;border-radius:13px;
  display:grid;place-items:center;cursor:pointer;border:1px solid var(--hair-2);
  background:linear-gradient(180deg,var(--panel-3),var(--panel));color:var(--ink-2);
  box-shadow:var(--shadow);transition:transform .16s,color .16s,border-color .16s,box-shadow .16s;}
.dvc-fab:hover{transform:translateY(-2px);color:var(--ink);border-color:rgba(var(--accent-rgb),.5);}
.dvc-fab.on{color:var(--accent);border-color:rgba(var(--accent-rgb),.6);box-shadow:var(--shadow),0 0 18px -6px rgba(var(--accent-rgb),.7);}
.dvc-fab svg{width:19px;height:19px;}
.dvc-fab .dvc-pulse{position:absolute;top:7px;right:7px;width:7px;height:7px;border-radius:50%;background:#ff5d7a;animation:dvcpulse 1.6s ease-in-out infinite;}
@keyframes dvcpulse{0%{box-shadow:0 0 0 0 rgba(255,93,122,.5);}70%{box-shadow:0 0 0 7px rgba(255,93,122,0);}100%{box-shadow:0 0 0 0 rgba(255,93,122,0);}}
.dvcon{position:fixed;right:18px;bottom:92px;z-index:91;width:720px;max-width:calc(100vw - 32px);
  max-height:calc(100vh - 148px);display:flex;flex-direction:column;overflow:hidden;
  background:var(--panel);border:1px solid var(--hair-2);border-radius:14px;
  box-shadow:var(--shadow),0 0 0 1px rgba(var(--accent-rgb),.04);font-family:var(--font-mono);
  transform-origin:bottom right;animation:dvcin .22s cubic-bezier(.2,.7,.2,1);}
@keyframes dvcin{from{opacity:0;transform:translateY(8px) scale(.985);}to{opacity:1;transform:none;}}
.dvc-head{flex:none;display:flex;align-items:center;gap:10px;padding:9px 11px 9px 13px;border-bottom:1px solid var(--hair);background:linear-gradient(180deg,var(--panel-3),var(--panel));}
.dvc-brand{display:flex;align-items:center;color:var(--accent);}
.dvc-tabs{display:flex;gap:2px;padding:2px;background:var(--panel-2);border:1px solid var(--hair);border-radius:8px;}
.dvc-tab{display:inline-flex;align-items:center;gap:6px;height:28px;font-family:var(--font-mono);font-size:11px;color:var(--ink-3);background:transparent;border:none;border-radius:6px;padding:0 10px;cursor:pointer;transition:all .12s;}
.dvc-tab:hover{color:var(--ink-2);}
.dvc-tab.on{color:var(--ink);background:var(--panel-3);box-shadow:0 0 0 1px var(--hair-2);}
.dvc-tab.on svg{color:var(--accent);}
.dvc-tab svg{width:13px;height:13px;color:var(--ink-4);}
.dvc-cnt{display:inline-flex;align-items:center;justify-content:center;height:15px;font-size:9px;padding:0 5px;border-radius:999px;background:var(--panel);border:1px solid var(--hair);color:var(--ink-3);min-width:16px;}
.dvc-tab.on .dvc-cnt{color:var(--ink-2);border-color:var(--hair-2);}
.dvc-live{display:inline-flex;align-items:center;gap:6px;font-size:9.5px;color:var(--ink-3);}
.dvc-ld{width:7px;height:7px;border-radius:50%;background:#35e0a1;position:relative;}
.dvc-ld::after{content:"";position:absolute;inset:-3px;border-radius:50%;background:inherit;opacity:.4;filter:blur(2px);}
.dvc-live.on .dvc-ld{animation:dvcblink 1.5s ease-in-out infinite;}
@keyframes dvcblink{0%,100%{opacity:1;}50%{opacity:.35;}}
.dvc-sp{flex:1;}
.dvc-ic{display:inline-flex;align-items:center;gap:6px;font-family:var(--font-mono);font-size:11px;color:var(--ink-2);background:transparent;border:1px solid var(--hair);border-radius:var(--r-sm);padding:5px 9px;cursor:pointer;transition:all .12s;}
.dvc-ic:hover{color:var(--ink);background:var(--panel-3);border-color:var(--hair-2);}
.dvc-ic.ok{color:var(--lat-g);border-color:color-mix(in oklch,var(--lat-g),transparent 55%);}
.dvc-ic svg{width:13px;height:13px;}
.dvc-x{border:none;padding:5px;color:var(--ink-3);background:transparent;border-radius:var(--r-sm);cursor:pointer;display:inline-flex;}
.dvc-x:hover{color:var(--ink);background:var(--panel-3);}
.dvc-x svg{width:13px;height:13px;}
.dvc-body{flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden;}
.dvc-scroll{flex:1;min-height:0;overflow:auto;}
.dvc-scroll::-webkit-scrollbar,.dvc-fl::-webkit-scrollbar{width:9px;}
.dvc-scroll::-webkit-scrollbar-thumb,.dvc-fl::-webkit-scrollbar-thumb{background:var(--hair-2);border-radius:6px;border:2px solid transparent;background-clip:padding-box;}
.dvc-tbl{min-width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums;}
.dvc-tbl thead th{position:sticky;top:0;z-index:2;background:var(--panel-2);border-bottom:1px solid var(--hair-2);font-weight:500;font-size:9.5px;letter-spacing:.06em;text-transform:uppercase;color:var(--ink-3);padding:8px 9px;text-align:right;white-space:nowrap;cursor:pointer;user-select:none;transition:color .12s,background .12s;}
.dvc-tbl thead th:first-child{text-align:left;padding-left:13px;}
.dvc-tbl thead th:last-child{padding-right:13px;}
.dvc-tbl thead th:hover{color:var(--ink-2);background:var(--panel-3);}
.dvc-tbl thead th.srt{color:var(--ink);}
.dvc-arr{display:inline-block;width:9px;margin-left:3px;color:var(--accent);font-size:8px;vertical-align:middle;}
.dvc-tbl tbody td{padding:0 9px;height:32px;border-bottom:1px solid var(--hair);text-align:right;white-space:nowrap;font-size:12px;color:var(--ink-2);}
.dvc-tbl tbody td:first-child{text-align:left;padding-left:13px;}
.dvc-tbl tbody td:last-child{padding-right:13px;}
.dvc-row{cursor:pointer;transition:background .1s;}
.dvc-row:hover{background:var(--panel-2);}
.dvc-row.open{background:var(--panel-2);}
.dvc-tw{width:11px;height:11px;flex:none;color:var(--ink-4);transition:transform .15s;}
.dvc-row.open .dvc-tw{transform:rotate(90deg);color:var(--accent);}
.dvc-lead{display:flex;align-items:center;gap:8px;color:var(--ink);font-weight:500;}
.dvc-nm{overflow:hidden;text-overflow:ellipsis;}
.dvc-id{color:var(--ink-4);font-weight:400;}
.dvc-lat .v{display:inline-block;padding:2px 6px;border-radius:5px;font-weight:500;}
.dvc-lat.g .v{color:var(--lat-g);}
.dvc-lat.a .v{color:var(--lat-a);background:var(--lat-a-bg);}
.dvc-lat.r .v{color:var(--lat-r);background:var(--lat-r-bg);font-weight:600;box-shadow:inset 0 0 0 1px var(--lat-r-ring);}
.dvc-lat.na .v{color:var(--ink-4);}
.dvc-dim{color:var(--ink-4);}
.dvc-err .v{display:inline-block;padding:2px 6px;border-radius:5px;}
.dvc-err.zero .v{color:var(--ink-4);}
.dvc-err.bad .v{color:var(--lat-r);background:var(--lat-r-bg);font-weight:600;box-shadow:inset 0 0 0 1px var(--lat-r-ring);}
.dvc-spk{text-align:right;padding-right:13px;}
.dvc-spk svg{display:inline-block;vertical-align:middle;}
.dvc-stat{display:inline-flex;align-items:center;gap:6px;justify-content:flex-end;}
.dvc-stat .lb{font-size:10px;letter-spacing:.06em;text-transform:uppercase;}
.dvc-needs{width:5px;height:5px;border-radius:50%;background:var(--st-blocked);flex:none;}
.dvc-tool{width:17px;height:17px;flex:none;border-radius:4px;display:inline-grid;place-items:center;}
.dvc-pchip{display:inline-flex;align-items:center;gap:6px;justify-content:flex-end;color:var(--ink-2);}
.dvc-pi{width:16px;height:16px;border-radius:4px;display:grid;place-items:center;flex:none;}
.dvc-pn{overflow:hidden;text-overflow:ellipsis;max-width:96px;}
.dvc-mtr{display:inline-block;width:46px;height:4px;border-radius:3px;background:var(--hair);overflow:hidden;vertical-align:middle;margin-right:6px;}
.dvc-mtr>i{display:block;height:100%;border-radius:3px;}
.dvc-branch{color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;max-width:120px;display:inline-block;vertical-align:bottom;}
.dvc-detail>td{padding:0;border-bottom:1px solid var(--hair);background:var(--bg);}
.dvc-din{padding:12px 14px 14px 32px;display:flex;flex-direction:column;gap:11px;animation:dvcexp .22s ease;}
@keyframes dvcexp{from{opacity:0;transform:translateY(-3px);}to{opacity:1;transform:none;}}
.dvc-kv{display:grid;grid-template-columns:repeat(auto-fill,minmax(210px,1fr));gap:5px 22px;}
.dvc-kv .r{display:flex;gap:8px;font-size:11px;align-items:baseline;}
.dvc-kv .k{color:var(--ink-4);min-width:74px;}
.dvc-kv .vv{color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-kv .vv b{color:var(--ink);font-weight:600;}
.dvc-sl{font-size:9.5px;letter-spacing:.1em;text-transform:uppercase;color:var(--ink-3);margin-bottom:3px;}
.dvc-files{display:flex;flex-direction:column;gap:1px;}
.dvc-frow{display:grid;grid-template-columns:16px 1fr auto;gap:9px;align-items:center;font-size:11px;padding:2px 0;color:var(--ink-2);}
.dvc-fp{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-task{font-size:11.5px;color:var(--ink-2);line-height:1.5;}
.dvc-attn{display:inline-flex;align-items:center;gap:6px;font-size:10.5px;padding:5px 9px;border-radius:6px;color:var(--lat-r);background:var(--lat-r-bg);border:1px solid var(--lat-r-ring);align-self:flex-start;}
.dvc-chip{display:inline-flex;align-items:center;gap:5px;font-size:10px;padding:2px 7px;border-radius:999px;border:1px solid var(--hair);color:var(--ink-2);background:var(--panel-2);}
.dvc-calls{display:flex;flex-direction:column;gap:1px;}
.dvc-cr{display:grid;grid-template-columns:84px 1fr auto 14px;align-items:center;gap:10px;font-size:11px;padding:3px 0;color:var(--ink-2);}
.dvc-cr .ts{color:var(--ink-4);} .dvc-cr .cm{color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-cr .du{text-align:right;font-weight:500;}
.dvc-cr .du.g{color:var(--lat-g);} .dvc-cr .du.a{color:var(--lat-a);} .dvc-cr .du.r{color:var(--lat-r);}
.dvc-dot{width:7px;height:7px;border-radius:50%;justify-self:center;}
.dvc-dot.ok{background:var(--lat-g);} .dvc-dot.er{background:var(--lat-r);box-shadow:0 0 0 2px var(--lat-r-bg);}
.dvc-dh{display:flex;align-items:center;gap:10px;font-size:10px;color:var(--ink-3);}
.dvc-dh .lbl{text-transform:uppercase;letter-spacing:.1em;}
.dvc-ds{display:flex;gap:14px;margin-left:auto;color:var(--ink-2);}
.dvc-ds b{color:var(--ink);font-weight:600;}
.dvc-feed{flex:none;border-top:1px solid var(--hair-2);}
.dvc-fh{display:flex;align-items:center;gap:9px;width:100%;padding:9px 13px;background:var(--panel-2);border:none;border-bottom:1px solid var(--hair);cursor:pointer;color:var(--ink-3);font-family:var(--font-mono);text-align:left;}
.dvc-fh:hover{color:var(--ink-2);}
.dvc-fh .dvc-tw.open{transform:rotate(90deg);color:var(--accent);}
.dvc-fh .lbl{font-size:9.5px;letter-spacing:.1em;text-transform:uppercase;color:var(--ink-3);}
.dvc-fh .ct{font-size:10px;color:var(--ink-4);margin-left:auto;}
.dvc-fl{padding:5px 13px 12px;max-height:240px;overflow-y:auto;}
.dvc-fr{display:grid;grid-template-columns:90px 16px 1fr auto 22px;align-items:center;gap:10px;font-size:11px;padding:4px 0;border-bottom:1px solid var(--hair);color:var(--ink-2);}
.dvc-fr:last-child{border-bottom:none;}
.dvc-fr .ts{color:var(--ink-4);} .dvc-fr .cm{color:var(--ink);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-fr .du{text-align:right;font-weight:500;}
.dvc-fr .du.g{color:var(--lat-g);} .dvc-fr .du.a{color:var(--lat-a);} .dvc-fr .du.r{color:var(--lat-r);}
.dvc-fr .ix{color:var(--ink-4);text-align:right;font-size:9px;}
.dvc-empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:13px;padding:54px 20px 60px;text-align:center;}
.dvc-ring{width:46px;height:46px;border-radius:50%;border:1.5px dashed var(--hair-2);display:grid;place-items:center;color:var(--ink-4);}
.dvc-empty h4{font-family:var(--font-disp);font-weight:600;font-size:14px;color:var(--ink-2);}
.dvc-empty p{font-size:11.5px;color:var(--ink-4);max-width:300px;line-height:1.55;}
.dvc-hint{display:inline-flex;align-items:center;gap:6px;font-size:10.5px;color:var(--ink-3);}
.dvc-hint .dvc-ld{animation:dvcblink 1.5s ease-in-out infinite;}
.dvc-foot{flex:none;display:flex;align-items:center;gap:14px;padding:8px 13px;border-top:1px solid var(--hair);background:var(--panel-2);font-size:10px;color:var(--ink-3);}
.dvc-leg{display:flex;align-items:center;gap:6px;}
.dvc-sw{width:9px;height:9px;border-radius:3px;}
.dvc-sw.g{background:var(--lat-g);} .dvc-sw.a{background:var(--lat-a);} .dvc-sw.r{background:var(--lat-r);}
`,
  ],
})
export class DevPanelComponent implements OnDestroy {
  readonly perf = inject(PerfStore);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly projectsStore = inject(ProjectsStore);
  private readonly host = inject(ElementRef<HTMLElement>);

  readonly dev = isDevMode();
  readonly open = signal(false);
  readonly tab = signal<"perf" | "agents" | "projects">("perf");
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
    // age out the 10s window while the panel is open
    this.tickIv = setInterval(() => {
      if (this.open() && this.tab() === "perf") this.perf.tick();
    }, 1000);
  }
  ngOnDestroy() {
    clearInterval(this.tickIv);
    clearTimeout(this.copiedTo);
  }

  @HostListener("document:keydown.escape") onEsc() {
    if (this.open()) this.open.set(false);
  }

  // close on click outside the FAB + panel (both live under this host element)
  @HostListener("document:mousedown", ["$event"]) onDocDown(e: MouseEvent) {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node)) this.open.set(false);
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
    return `${rows.length} commands · ${calls} calls/10s · ${slow} >100ms` + (this.dev ? "" : " · prod (aggregates only)");
  });
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
  sparkPoints(data: number[]): string {
    if (!data.length) return "";
    const w = 58, h = 16, max = Math.max(...data, 1);
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
    const m: Record<string, string> = { running: "var(--st-running)", blocked: "var(--st-blocked)", done: "var(--st-done)", waiting: "var(--accent-2)", queued: "var(--ink-3)", idle: "var(--ink-4)" };
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
}
