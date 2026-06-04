import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent, Project } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { STATUS_META } from "../utils";

interface AgentNode {
  ag: Agent;
  y: number;
}
interface Laid {
  p: Project;
  projY: number;
  agentNodes: AgentNode[];
}

@Component({
  selector: "app-graph-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let g = layout();
    <div style="padding:18px;display:grid;place-items:center;min-height:0;overflow:auto">
      <svg [attr.viewBox]="'0 0 ' + W + ' ' + g.H" style="width:100%;max-width:920px;height:auto">
        <defs>
          <radialGradient id="orch-core" cx="50%" cy="50%">
            <stop offset="0" stop-color="var(--accent)" stop-opacity="0.4" />
            <stop offset="1" stop-color="var(--accent)" stop-opacity="0" />
          </radialGradient>
        </defs>

        <!-- root → project edges -->
        @for (l of g.laid; track l.p.id) {
          <path
            [attr.d]="rootEdge(l.projY, g.rootY)"
            fill="none"
            [attr.stroke]="l.p.color"
            stroke-width="1.6"
            stroke-opacity="0.5"
          />
        }

        <!-- project → agent edges -->
        @for (l of g.laid; track l.p.id) {
          @for (n of l.agentNodes; track n.ag.id) {
            <path
              [attr.d]="agentEdge(l.projY, n.y)"
              fill="none"
              [attr.stroke]="color(n.ag)"
              stroke-width="1.4"
              [attr.stroke-opacity]="n.ag.status === 'queued' ? 0.22 : 0.5"
              [attr.stroke-dasharray]="n.ag.status === 'running' ? '4 4' : null"
            >
              @if (n.ag.status === 'running') {
                <animate attributeName="stroke-dashoffset" from="16" to="0" dur="0.7s" repeatCount="indefinite" />
              }
            </path>
          }
        }

        <!-- root node -->
        <circle [attr.cx]="xRoot" [attr.cy]="g.rootY" r="40" fill="url(#orch-core)" />
        <circle [attr.cx]="xRoot" [attr.cy]="g.rootY" r="20" fill="var(--panel-3)" stroke="var(--accent)" stroke-width="1.6" />
        <text [attr.x]="xRoot" [attr.y]="g.rootY + 1" text-anchor="middle" font-family="var(--font-disp)" font-size="9" font-weight="700" fill="var(--ink)">ORCH</text>
        <text [attr.x]="xRoot" [attr.y]="g.rootY + 36" text-anchor="middle" font-family="var(--font-mono)" font-size="9" fill="var(--ink-3)">{{ store.org }}</text>

        <!-- project nodes -->
        @for (l of g.laid; track l.p.id) {
          <g>
            <rect [attr.x]="xProj - 16" [attr.y]="l.projY - 16" width="32" height="32" rx="9" fill="var(--panel-3)" [attr.stroke]="l.p.color" stroke-width="1.6" />
            <text [attr.x]="xProj" [attr.y]="l.projY + 5" text-anchor="middle" font-family="var(--font-disp)" font-size="13" font-weight="700" [attr.fill]="l.p.color">{{ l.p.name[0].toUpperCase() }}</text>
            <text [attr.x]="xProj + 26" [attr.y]="l.projY - 2" font-family="var(--font-disp)" font-size="12" font-weight="600" fill="var(--ink)">{{ l.p.name }}</text>
            <text [attr.x]="xProj + 26" [attr.y]="l.projY + 11" font-family="var(--font-mono)" font-size="9" fill="var(--ink-3)">{{ l.p.branch }} · {{ l.p.head }}</text>
          </g>
        }

        <!-- agent nodes -->
        @for (l of g.laid; track l.p.id) {
          @for (n of l.agentNodes; track n.ag.id) {
            <g style="cursor:pointer" (click)="store.openAgent(n.ag.id)">
              <circle [attr.cx]="xAgent" [attr.cy]="n.y" r="8" fill="var(--panel-3)" [attr.stroke]="color(n.ag)" stroke-width="2" />
              <circle [attr.cx]="xAgent" [attr.cy]="n.y" r="3.2" [attr.fill]="color(n.ag)">
                @if (n.ag.status === 'running') {
                  <animate attributeName="r" values="3.2;5;3.2" dur="1.4s" repeatCount="indefinite" />
                }
              </circle>
              <text [attr.x]="xAgent + 16" [attr.y]="n.y - 1" font-family="var(--font-disp)" font-size="12" font-weight="600" fill="var(--ink)">{{ n.ag.name }}</text>
              <text [attr.x]="xAgent + 16" [attr.y]="n.y + 12" font-family="var(--font-mono)" font-size="9" fill="var(--ink-3)">{{ label(n.ag) }} · {{ n.ag.commits }}c · {{ n.ag.branch.replace('agent/', '') }}</text>
            </g>
          }
          @if (!l.agentNodes.length) {
            <text [attr.x]="xAgent" [attr.y]="l.projY + 4" font-family="var(--font-mono)" font-size="10" fill="var(--ink-4)">no agents</text>
          }
        }
      </svg>
    </div>
  `,
})
export class GraphViewComponent {
  readonly store = inject(OrchestraStore);
  readonly agents = input.required<Agent[]>();
  readonly projects = input.required<Project[]>();

  readonly rowH = 50;
  readonly W = 780;
  readonly xRoot = 54;
  readonly xProj = 250;
  readonly xAgent = 470;

  readonly layout = computed(() => {
    let y = 26;
    const laid: Laid[] = this.projects().map((p) => {
      const ags = this.agents().filter((a) => a.projectId === p.id);
      const count = Math.max(ags.length, 1);
      const top = y;
      const agentNodes = ags.map((ag, i) => ({ ag, y: top + i * this.rowH + this.rowH / 2 }));
      const projY = top + (count * this.rowH) / 2;
      y += count * this.rowH + 18;
      return { p, projY, agentNodes };
    });
    const H = y + 8;
    return { laid, H, rootY: H / 2 };
  });

  color(ag: Agent): string {
    return STATUS_META[ag.status].color;
  }
  label(ag: Agent): string {
    return STATUS_META[ag.status].label;
  }
  rootEdge(projY: number, rootY: number): string {
    const mid = (this.xRoot + this.xProj) / 2;
    return `M${this.xRoot + 22},${rootY} C${mid},${rootY} ${mid},${projY} ${this.xProj - 18},${projY}`;
  }
  agentEdge(projY: number, ny: number): string {
    const mid = (this.xProj + this.xAgent) / 2;
    return `M${this.xProj + 16},${projY} C${mid},${projY} ${mid},${ny} ${this.xAgent - 10},${ny}`;
  }
}
