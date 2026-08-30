import { Agent, Project } from "../models";

/**
 * v2 project tabs: the frontend pseudo-agent for a project's MAIN worktree —
 * "an agent is just a worktree with a process attached", and a project tab is
 * that worktree with a plain shell. Synthesized at render time (never stored,
 * never in AgentsStore); its id IS the project id, which the backend resolves
 * to the same pseudo view (AgentService::project_pseudo_record), so every
 * worktree-scoped command works unchanged.
 */
export function pseudoProjectAgent(p: Project, shellRunning: boolean): Agent {
  const branch = p.branch ?? p.defaultBranch ?? "main";
  return {
    id: p.id,
    projectId: p.id,
    tool: "shell",
    model: "",
    name: p.name,
    task: "",
    status: shellRunning ? "running" : "idle",
    branch,
    worktree: p.path,
    base: branch,
    commits: 0,
    elapsed: 0,
    progress: 0,
    pending: [],
  };
}
