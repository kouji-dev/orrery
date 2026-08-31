import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import {
  LucideAngularModule,
  type LucideIconData,
  Archive,
  ArrowDownToLine,
  ArrowLeftRight,
  ArrowUpFromLine,
  Bell,
  Bot,
  Box,
  ChartGantt,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsDown,
  ChevronsUp,
  CircleQuestionMark,
  Clock,
  Columns2,
  Columns3,
  Copy,
  CornerDownLeft,
  Cpu,
  Database,
  Ellipsis,
  EllipsisVertical,
  ExternalLink,
  File,
  FileDiff,
  Flag,
  Folder,
  FolderOpen,
  GitBranch,
  GitCommitHorizontal,
  GitGraph,
  GitMerge,
  GitPullRequest,
  Globe,
  Layers,
  LayoutGrid,
  Link,
  Lock,
  MessageSquare,
  Moon,
  Network,
  PanelLeft,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Rocket,
  RotateCcw,
  Rows2,
  Search,
  Server,
  Settings,
  Shield,
  Sparkle,
  Sparkles,
  Square,
  Sun,
  Tag,
  Terminal,
  Trash2,
  Volume2,
  X,
  Zap,
} from "lucide-angular";

// Maps the app's icon names → Lucide icon data. The `<app-icon name="…">` API is
// unchanged, so every call site keeps working; only the rendered glyph set swaps
// to Lucide. AI/tool brand marks (claude/codex/cursor/gemini) live in
// tool-icons.ts and are intentionally NOT part of this set.
const LUCIDE: Record<string, LucideIconData> = {
  agent: Bot,
  sparkles: Sparkles,
  branch: GitBranch,
  terminal: Terminal,
  diff: FileDiff,
  chat: MessageSquare,
  git: GitGraph,
  commit: GitCommitHorizontal,
  bell: Bell,
  play: Play,
  pause: Pause,
  plus: Plus,
  check: Check,
  x: X,
  chevron: ChevronRight,
  chevronD: ChevronDown,
  chevsUp: ChevronsUp,
  chevsDown: ChevronsDown,
  folder: Folder,
  file: File,
  clock: Clock,
  bolt: Zap,
  search: Search,
  sun: Sun,
  moon: Moon,
  tag: Tag,
  merge: GitMerge,
  layers: Layers,
  grid: LayoutGrid,
  columns: Columns3,
  timeline: ChartGantt,
  graph: Network,
  dots: Ellipsis,
  stop: Square,
  refresh: RefreshCw,
  cpu: Cpu,
  link: Link,
  flag: Flag,
  spark: Sparkle,
  box: Box,
  globe: Globe,
  server: Server,
  database: Database,
  cube: Box,
  rocket: Rocket,
  archive: Archive,
  trash: Trash2,
  rename: Pencil,
  dup: Copy,
  ext: ExternalLink,
  push: ArrowUpFromLine,
  pr: GitPullRequest,
  stage: ArrowDownToLine,
  discard: RotateCcw,
  dotsV: EllipsisVertical,
  folderOpen: FolderOpen,
  enter: CornerDownLeft,
  question: CircleQuestionMark,
  shield: Shield,
  splitCol: Columns2,
  splitRow: Rows2,
  panelLeft: PanelLeft,
  swap: ArrowLeftRight,
  settings: Settings,
  lock: Lock,
  volume: Volume2,
};

/** Type-relative icon steps; see the --ico-* scale in styles.css. */
export type IconSize = "xs" | "sm" | "md" | "lg" | "xl";

@Component({
  selector: "app-icon",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [LucideAngularModule],
  host: {
    style: "display:inline-flex;line-height:0",
    "[style.color]": "color() || null",
    "[style.--ico]": "dim()",
  },
  template: `
    <lucide-angular [img]="icon()" [size]="NOMINAL" [strokeWidth]="1.8" style="display:block" />
  `,
})
export class IconComponent {
  readonly name = input.required<string>();
  readonly size = input<IconSize>("md");
  readonly color = input<string | null>(null);

  /**
   * lucide writes width/height as SVG attributes; the real size comes from the
   * `app-icon svg` rule in styles.css reading `--ico`. This nominal value only
   * has to be large enough that nothing depends on it.
   */
  protected readonly NOMINAL = 24;

  // Unknown names fall back to the ellipsis glyph (the old map fell back to "dots").
  readonly icon = computed<LucideIconData>(() => LUCIDE[this.name()] ?? Ellipsis);

  /** Resolved CSS length for `--ico`: an em token from the size scale. */
  readonly dim = computed(() => `var(--ico-${this.size()})`);
}
