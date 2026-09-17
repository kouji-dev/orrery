/* global React, Icon, StatusDot, STATUS_META, fileName, fileDir, FileTree, projectOf, AgentCommitHistory, AgentConflictCard, AGENT_GIT */
// ORCHESTRA right panel — Files · Inbox · Git, scoped to the selected agent

const { useState: useStateR } = React;

// ---------------- Files ----------------
function FilesTab({ agent, project, onOpenFile, activePath }) {
  if (!agent) return <EmptyR icon="folder" text="Select an agent to browse its worktree" />;
  return <FileTree project={project} agent={agent} onOpenFile={(p) => onOpenFile(agent.id, p)} activePath={activePath} />;
}

// ---------------- Inbox ----------------
const KIND_META = {
  permission: { icon: "bolt", color: "#f5c451", verb: "wants to run" },
  decision: { icon: "flag", color: "var(--st-blocked)", verb: "needs a decision" },
  review: { icon: "merge", color: "var(--st-done)", verb: "ready for review" },
};

function PendingCard({ agent, item, onResolve }) {
  const m = KIND_META[item.kind] || KIND_META.permission;
  return (
    <div className="rise" style={{ margin: "8px 10px", padding: 11, borderRadius: "var(--r-md)",
      background: "var(--panel-2)", border: "1px solid color-mix(in oklch, " + m.color + ", transparent 70%)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 7 }}>
        <Icon name={m.icon} size="sm" style={{ color: m.color, flex: "none" }} />
        <span style={{ fontSize: 11.5, fontWeight: 600, color: "var(--ink)" }}>{item.title}</span>
        <span style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{item.when}</span>
      </div>
      <div style={{ fontSize: 10.5, color: "var(--ink-3)", marginBottom: 9 }}>
        <span style={{ color: m.color }}>{agent.name}</span> {m.verb}
      </div>
      <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--ink-2)", padding: "7px 9px",
        background: "var(--bg)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", marginBottom: 9, wordBreak: "break-all" }}>
        {item.kind === "permission" ? "$ " : ""}{item.cmd}
      </div>
      <div style={{ display: "flex", gap: 7, flexWrap: "wrap" }}>
        {item.kind === "permission" && <>
          <button className="btn primary" style={{ flex: 1, justifyContent: "center" }} onClick={() => onResolve(agent.id, item, "allow")}><Icon name="check" size="sm" />Allow</button>
          <button className="btn ghost-hair" style={{ flex: 1, justifyContent: "center" }} onClick={() => onResolve(agent.id, item, "deny")}><Icon name="x" size="sm" />Deny</button>
          <button className="btn ghost-hair" onClick={() => onResolve(agent.id, item, "always")} title="Always allow this command">∞</button>
        </>}
        {item.kind === "decision" && (item.options || ["Approve"]).map((opt) => (
          <button key={opt} className="btn ghost-hair" style={{ flex: 1, justifyContent: "center" }} onClick={() => onResolve(agent.id, item, opt)}>{opt}</button>
        ))}
        {item.kind === "review" && <>
          <button className="btn primary" style={{ flex: 1, justifyContent: "center" }} onClick={() => onResolve(agent.id, item, "merge")}><Icon name="merge" size="sm" />Merge</button>
          <button className="btn ghost-hair" style={{ flex: 1, justifyContent: "center" }} onClick={() => onResolve(agent.id, item, "diff")}><Icon name="diff" size="sm" />Review diff</button>
        </>}
      </div>
    </div>
  );
}

function InboxTab({ agents, scopeAgent, onResolve, onOpen }) {
  const list = scopeAgent ? [scopeAgent] : agents;
  const items = [];
  list.forEach((a) => (a.pending || []).forEach((p) => items.push({ agent: a, item: p })));
  if (!items.length) return <EmptyR icon="bell" text={scopeAgent ? "No pending actions for " + scopeAgent.name : "Inbox zero — no pending actions"} />;
  return (
    <div className="scroll-y" style={{ flex: 1, paddingBottom: 8 }}>
      {!scopeAgent && <div className="up" style={{ fontSize: 9, color: "var(--ink-3)", padding: "10px 14px 2px" }}>All projects · {items.length} pending</div>}
      {items.map(({ agent, item }, i) => <PendingCard key={i} agent={agent} item={item} onResolve={onResolve} />)}
    </div>
  );
}

// ---------------- Git ----------------
function GitTab({ agent, project, commits, agents, onOpen, onAct, onOpenGit, onOpenFile }) {
  const [staged, setStaged] = useStateR(false);
  if (!agent) {
    // global commit feed
    return (
      <div className="scroll-y" style={{ flex: 1, padding: "8px 0" }}>
        <div className="up" style={{ fontSize: 9, color: "var(--ink-3)", padding: "6px 14px" }}>Commit feed · all worktrees</div>
        <CommitFeed commits={commits} agents={agents} onOpen={onOpen} />
      </div>
    );
  }
  const totAdd = agent.files.reduce((s, f) => s + f.add, 0);
  const totDel = agent.files.reduce((s, f) => s + f.del, 0);
  const hasConflict = window.AGENT_GIT && AGENT_GIT.agentHasConflict(agent);
  const openCommit = (sha, path) => onOpenGit && onOpenGit(agent.id, { kind: "commit", sha, path });
  const openRange = (shas) => onOpenGit && onOpenGit(agent.id, { kind: "range", shas });
  const openFileHistory = (path) => onOpenGit && onOpenGit(agent.id, { kind: "filehistory", path });
  return (
    <div className="scroll-y" style={{ flex: 1 }}>
      {/* branch header */}
      <div style={{ padding: "10px 14px", borderBottom: "1px solid var(--hair)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 4 }}>
          <Icon name="branch" size="sm" style={{ color: "var(--accent-2)" }} />
          <span style={{ fontSize: 11.5, color: "var(--ink)" }}>{agent.branch}</span>
        </div>
        <div style={{ fontSize: 10, color: "var(--ink-4)", display: "flex", gap: 8 }} className="tnum">
          <span>base {agent.base}</span><span>·</span>
          <span>{agent.commits} ahead</span><span>·</span>
          <span style={{ color: "var(--code-add-ink)" }}>+{totAdd}</span>
          <span style={{ color: "var(--code-del-ink)" }}>−{totDel}</span>
        </div>
      </div>

      {/* merge conflict surface (only when the branch merge is blocked) */}
      {hasConflict && <AgentConflictCard ag={agent} project={project} onOpenConflict={() => onOpenGit && onOpenGit(agent.id, { kind: "conflict" })} />}

      {/* working tree status */}
      <div style={{ padding: "10px 14px 6px" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>{staged ? "Staged changes" : "Changes"}</span>
          <span className="tnum" style={{ fontSize: 9, color: "var(--ink-4)" }}>{agent.files.length}</span>
        </div>
      </div>
      {agent.files.length ? agent.files.map((f, i) => (
        <div key={i} className="git-file-row" onClick={() => onOpenFile && onOpenFile(agent.id, f.path)}
          style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 8px 4px 14px", fontSize: 11, cursor: "pointer", margin: "0 6px", borderRadius: "var(--r-sm)" }}
          onMouseEnter={(e) => { e.currentTarget.style.background = "var(--panel-2)"; e.currentTarget.querySelector(".fh-btn").style.opacity = 1; }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.querySelector(".fh-btn").style.opacity = 0; }}>
          <span style={{ flex: "none", width: 12, textAlign: "center", fontSize: 9, fontWeight: 700,
            color: f.state === "A" ? "var(--code-add-ink)" : f.state === "D" ? "var(--code-del-ink)" : "var(--accent-2)" }}>{f.state}</span>
          <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: staged ? "var(--ink-2)" : "var(--ink)" }} title={f.path}>
            <span style={{ color: "var(--ink-4)" }}>{fileDir(f.path)}</span>{fileName(f.path)}
          </span>
          <button className="fh-btn pane-btn" title="File history" onClick={(e) => { e.stopPropagation(); openFileHistory(f.path); }} style={{ flex: "none", opacity: 0 }}>
            <Icon name="clock" size="sm" style={{ width: 12, height: 12 }} />
          </button>
          <span className="tnum" style={{ flex: "none", fontSize: 9.5, display: "flex", gap: 4 }}>
            <span style={{ color: "var(--code-add-ink)" }}>+{f.add}</span>
            {f.del > 0 && <span style={{ color: "var(--code-del-ink)" }}>−{f.del}</span>}
          </span>
        </div>
      )) : <div style={{ padding: "4px 14px 8px", fontSize: 10.5, color: "var(--ink-4)" }}>clean — no working changes</div>}

      {/* git action buttons */}
      <div style={{ padding: 12, display: "grid", gap: 7 }}>
        {agent.files.length > 0 && (
          <button className="btn ghost-hair" onClick={() => setStaged((s) => !s)} style={{ justifyContent: "flex-start" }}>
            <Icon name="stage" size="sm" />{staged ? "Unstage all" : "Stage all changes"}
          </button>
        )}
        <button className="btn ghost-hair" disabled={agent.files.length === 0} onClick={() => onAct(agent.id, "commit")} style={{ justifyContent: "flex-start" }}>
          <Icon name="commit" size="sm" />Commit {staged ? "staged" : "all"}
        </button>
        <button className="btn ghost-hair" disabled={agent.commits === 0} onClick={() => onAct(agent.id, "push")} style={{ justifyContent: "flex-start" }}>
          <Icon name="push" size="sm" />Push to origin
        </button>
        <button className="btn ghost-hair" disabled={agent.commits === 0} onClick={() => onAct(agent.id, "pr")} style={{ justifyContent: "flex-start" }}>
          <Icon name="pr" size="sm" />Open pull request
        </button>
        <button className="btn primary" disabled={agent.commits === 0} onClick={() => onAct(agent.id, "merge")} style={{ justifyContent: "center" }}>
          <Icon name="merge" size="sm" />Merge {agent.branch.replace("agent/", "")} → {project ? project.branch : "main"}
        </button>
        <button className="btn ghost-hair" disabled={agent.files.length === 0} onClick={() => onAct(agent.id, "discard")} style={{ justifyContent: "flex-start", color: "var(--st-blocked)" }}>
          <Icon name="discard" size="sm" />Discard working changes
        </button>
      </div>

      {/* this branch's commits — expandable + range-selectable, opens diff in center */}
      <AgentCommitHistory ag={agent} onOpenCommit={openCommit} onOpenRange={openRange} onOpenFileHistory={openFileHistory} />
    </div>
  );
}

function CommitFeed({ commits, agents, onOpen, compact }) {
  return (
    <div style={{ position: "relative", paddingLeft: 26 }}>
      <div style={{ position: "absolute", left: 18, top: 8, bottom: 8, width: 1, background: "var(--hair)" }} />
      {commits.map((c, i) => {
        const ag = agents.find((a) => a.id === c.agent);
        const col = ag ? STATUS_META[ag.status].color : "var(--ink-3)";
        return (
          <div key={i} onClick={() => ag && onOpen(ag.id)}
            style={{ position: "relative", padding: "7px 14px 7px 8px", cursor: "pointer", borderRadius: "var(--r-sm)", margin: "0 6px" }}
            onMouseEnter={(e) => e.currentTarget.style.background = "var(--panel-2)"}
            onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
            <span style={{ position: "absolute", left: -12, top: 11, width: 9, height: 9, borderRadius: "50%", background: "var(--panel)", border: "2px solid " + col }} />
            <div style={{ fontSize: 11.5, color: "var(--ink)", lineHeight: 1.4, textWrap: "pretty" }}>{c.msg}</div>
            <div style={{ display: "flex", alignItems: "center", gap: 7, marginTop: 3, fontSize: 9.5, color: "var(--ink-4)" }} className="tnum">
              {!compact && <span style={{ color: col }}>{ag ? ag.name : c.agent}</span>}
              <span className="chip" style={{ fontSize: 9, padding: "0 5px" }}>{c.sha}</span>
              <span>{c.files} files</span>
              <span style={{ marginLeft: "auto" }}>{c.when} ago</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function EmptyR({ icon, text }) {
  return (
    <div style={{ flex: 1, display: "grid", placeItems: "center", padding: 24 }}>
      <div style={{ textAlign: "center", color: "var(--ink-4)" }}>
        <Icon name={icon} size="lg" style={{ opacity: 0.5 }} />
        <div style={{ fontSize: 11, marginTop: 8, maxWidth: 180, lineHeight: 1.5 }}>{text}</div>
      </div>
    </div>
  );
}

function RightPanel({ agents, scopeAgent, onOpen, onResolve, onAct, commits, onOpenFile, openFile, onOpenGit }) {
  const [tab, setTab] = useStateR("files");
  const project = scopeAgent ? projectOf(scopeAgent.projectId) : null;
  const pendingCount = scopeAgent
    ? (scopeAgent.pending || []).length
    : agents.reduce((s, a) => s + (a.pending ? a.pending.length : 0), 0);

  const TABS = [
    { key: "files", icon: "folder", label: "Files" },
    { key: "inbox", icon: "bell", label: "Inbox", badge: pendingCount },
    { key: "git", icon: "git", label: "Git" },
  ];
  return (
    <aside style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel)", borderLeft: "1px solid var(--hair)" }}>
      {/* scope header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "0 12px", height: 38, borderBottom: "1px solid var(--hair)" }}>
        {scopeAgent ? <>
          <StatusDot status={scopeAgent.status} />
          <span style={{ fontSize: 11.5, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{scopeAgent.name}</span>
          {project && <span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: project.color, borderColor: "color-mix(in oklch, " + project.color + ", transparent 65%)" }}>{project.name}</span>}
        </> : <>
          <Icon name="layers" size="sm" style={{ color: "var(--accent)" }} />
          <span style={{ fontSize: 11.5, fontWeight: 600, color: "var(--ink-2)" }}>All projects</span>
        </>}
      </div>
      {/* tab bar */}
      <div style={{ display: "flex", alignItems: "center", padding: "0 6px", borderBottom: "1px solid var(--hair)", height: 36 }}>
        {TABS.map((t) => (
          <button key={t.key} className="btn" onClick={() => setTab(t.key)}
            style={{ padding: "7px 10px", borderRadius: 0, position: "relative", flex: 1, justifyContent: "center", color: tab === t.key ? "var(--ink)" : "var(--ink-3)" }}>
            {tab === t.key && <span style={{ position: "absolute", left: 8, right: 8, bottom: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
            <Icon name={t.icon} size="sm" style={{ color: tab === t.key ? "var(--accent)" : "inherit" }} />
            <span style={{ fontSize: 11 }}>{t.label}</span>
            {t.badge ? <span className="chip tnum" style={{ fontSize: 9, padding: "0 5px", color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 60%)" }}>{t.badge}</span> : null}
          </button>
        ))}
      </div>
      {tab === "files" && <FilesTab agent={scopeAgent} project={project} onOpenFile={onOpenFile} activePath={openFile && scopeAgent && openFile.agentId === scopeAgent.id ? openFile.path : null} />}
      {tab === "inbox" && <InboxTab agents={agents} scopeAgent={scopeAgent} onResolve={onResolve} onOpen={onOpen} />}
      {tab === "git" && <GitTab agent={scopeAgent} project={project} commits={commits} agents={agents} onOpen={onOpen} onAct={onAct} onOpenGit={onOpenGit} onOpenFile={onOpenFile} />}
    </aside>
  );
}

Object.assign(window, { RightPanel });
