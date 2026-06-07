import { inject, Injectable } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import {
  ActivityKind,
  Agent,
  AgentFile,
  Commit,
  FileDiff,
  FileNode,
  PermissionQuestion,
  PermissionSuggestion,
} from "../models";
import { bindFacade } from "../state/entity-facade";
import { createEntityStore } from "../state/entity-store";

/**
 * Backend-backed source of truth for agent identity/config/status.
 * Live runtime metrics (elapsed/working/files) are an overlay kept in
 * AgentRuntimeService and merged over these records.
 */
@Injectable({ providedIn: "root" })
export class AgentsStore {
  private bridge = inject(BRIDGE);
  private store = createEntityStore<Agent>((a) => a.id);

  readonly all = this.store.all;
  readonly loading = this.store.loading;

  private facade = bindFacade(this.store, this.bridge, {
    listCommand: Commands.AgentList,
    events: {
      created: Events.AgentCreated,
      updated: Events.AgentUpdated,
      deleted: Events.AgentDeleted,
    },
  });

  constructor() {
    void this.init();
  }
  private async init() {
    try {
      await this.facade.listen();
      await this.facade.load();
    } catch {
      // backend unavailable — start empty
    }
  }

  // ---- mutations: invoke only; the store updates from agent:// events ----
  spawn(req: {
    projectId: string;
    tool: string;
    model: string;
    effort: string | null;
    name: string;
    task: string;
    base: string;
  }): Promise<Agent> {
    return this.bridge.invoke<Agent>(Commands.AgentSpawn, { req });
  }
  update(
    id: string,
    patch: { status?: string; task?: string; model?: string; name?: string },
  ): Promise<Agent> {
    return this.bridge.invoke<Agent>(Commands.AgentUpdate, { id, req: patch });
  }
  async remove(id: string): Promise<void> {
    await this.bridge.invoke(Commands.AgentRemove, { id });
  }
  detectTools(): Promise<Array<{ id: string; available: boolean }>> {
    return this.bridge.invoke(Commands.DetectTools);
  }
  /** Worktree file tree (source recursed, ignored dirs as lazy stubs). */
  tree(id: string): Promise<FileNode[]> {
    return this.bridge.invoke<FileNode[]>(Commands.AgentTree, { id });
  }
  /** Immediate children of one directory — to lazily expand an unloaded folder. */
  listDir(id: string, path: string): Promise<FileNode[]> {
    return this.bridge.invoke<FileNode[]>(Commands.AgentDir, { id, path });
  }
  /** Working-tree changes in the agent's worktree (git status). */
  changes(id: string): Promise<AgentFile[]> {
    return this.bridge.invoke<AgentFile[]>(Commands.AgentChanges, { id });
  }
  /** Commits on the agent's branch (worktree HEAD log), tagged with the agent id. */
  commits(id: string): Promise<Commit[]> {
    return this.bridge.invoke<Commit[]>(Commands.AgentCommits, { id });
  }
  /** Commit selected paths (empty = all) in the worktree; resolves the short sha. */
  commit(id: string, message: string, paths: string[]): Promise<string> {
    return this.bridge.invoke<string>(Commands.AgentCommit, { id, message, paths });
  }
  /** Discard selected paths (empty = all) in the worktree. */
  discard(id: string, paths: string[]): Promise<void> {
    return this.bridge.invoke(Commands.AgentDiscard, { id, paths });
  }
  /** Merge the agent's branch into the source project's branch. */
  merge(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentMerge, { id });
  }
  /** Old/new content of a file for the diff view. */
  diff(id: string, path: string): Promise<FileDiff> {
    return this.bridge.invoke<FileDiff>(Commands.AgentDiff, { id, path });
  }
  /** Start watching an agent's worktree for changes (replaces any previous watch). */
  watch(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentWatch, { id });
  }
  /** Subscribe to worktree-changed events. Resolves an unsubscribe fn. */
  onWorktreeChanged(cb: (id: string) => void): Promise<() => void> {
    return this.bridge.on<{ id: string }>(Events.AgentChanged, (p) => cb(p.id));
  }

  /** Launch the agent's tool process (PTY-streamed), sized to the visible terminal.
   *  `resume` requests a "Continue session" launch (`claude --resume <sessionId>`)
   *  when the agent has a captured session id; defaults to a normal Start/Resume. */
  start(id: string, rows = 0, cols = 0, resume = false): Promise<void> {
    return this.bridge.invoke(Commands.AgentStart, { id, rows, cols, resume });
  }
  /** Stop the agent's running process. */
  stop(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentStop, { id });
  }
  /** Forward terminal keystrokes into the agent's PTY stdin. */
  input(id: string, data: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentInput, { id, data });
  }
  /** Approve the agent's pending permission prompt (tool-correct keystrokes). */
  allow(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentAllow, { id });
  }
  /** Deny the agent's pending permission prompt (tool-correct keystrokes). */
  deny(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentDeny, { id });
  }
  /** Select option `choice` (1-based) in the agent's pending numbered SELECT
   *  prompt (best-effort PTY keystrokes; assumes a numbered TUI select). */
  decide(id: string, choice: number): Promise<void> {
    return this.bridge.invoke(Commands.AgentDecide, { id, choice });
  }
  /** Resize the agent's PTY to match the visible terminal. */
  resize(id: string, rows: number, cols: number): Promise<void> {
    return this.bridge.invoke(Commands.AgentResize, { id, rows, cols });
  }
  /** Subscribe to streamed process output. */
  onOutput(cb: (id: string, chunk: string) => void): Promise<() => void> {
    return this.bridge.on<{ id: string; chunk: string }>(Events.AgentOutput, (p) => cb(p.id, p.chunk));
  }
  /** Subscribe to process-exit events. */
  onExit(cb: (id: string) => void): Promise<() => void> {
    return this.bridge.on<{ id: string }>(Events.AgentExit, (p) => cb(p.id));
  }

  /** Subscribe to hook-driven needs-input signals (the agent wants the user).
   *  Carries the full permission detail: tool + mode + command/description/
   *  filePath + suggested settings rules (suggestions is [] for non-Claude tools),
   *  plus a human `summary` headline and, for AskUserQuestion-style prompts, the
   *  structured `questions` (each with optional header + options). */
  onPermission(
    cb: (p: {
      agentId: string;
      tool: string;
      mode?: string;
      command?: string;
      description?: string;
      filePath?: string;
      suggestions?: PermissionSuggestion[];
      summary?: string;
      questions?: PermissionQuestion[];
    }) => void,
  ): Promise<() => void> {
    return this.bridge.on(Events.AgentPermission, cb);
  }
  /** Subscribe to non-blocking hook status pings (working / idle). */
  onStatus(cb: (id: string, state: string) => void): Promise<() => void> {
    return this.bridge.on<{ id: string; state: string }>(Events.AgentStatus, (p) =>
      cb(p.id, p.state),
    );
  }
  /** Subscribe to action-carrying activity from hooks (e.g. "Bash: npm test").
   *  The payload carries the precise hook `event` (e.g. "PreToolUse") so the
   *  runtime can log/branch on which hook produced the detail, plus a `kind`
   *  (user/agent/tool/success/error/question/info) used to colorize the preview. */
  onActivity(
    cb: (id: string, detail: string, event: string, kind: ActivityKind) => void,
  ): Promise<() => void> {
    return this.bridge.on<{
      agentId: string;
      tool: string;
      event: string;
      kind: ActivityKind;
      detail: string;
    }>(Events.AgentActivity, (p) => cb(p.agentId, p.detail, p.event, p.kind));
  }
}
