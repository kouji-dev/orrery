/* global React, Icon, fileName, fileDir, ReviewCode, diffToRows, AuthorChip */
// Orrery — GitHub pull requests (roadmap B7.1). The PR diff reuses the same
// inline-review surface built for agent diffs; checks and reviews are inline.

const { useState: useStateGh, useMemo: useMemoGh } = React;

window.GITHUB_PRS = window.GITHUB_PRS || [
  {
    n: 412, title: "feat: retry stripe webhooks w/ backoff", author: "ag_retry", branch: "agent/stripe-retry", base: "main",
    state: "open", draft: false, rel: "6h", additions: 188, deletions: 8, comments: 3, approvals: 1, reviewers: ["mara", "devin"],
    sha: "1f8c2a9",
    checks: [{ name: "ci / typecheck", state: "pass", ms: 42000 }, { name: "ci / unit", state: "pass", ms: 96000 }, { name: "ci / e2e", state: "running", ms: 0 }, { name: "codeql", state: "pass", ms: 214000 }],
    body: "withRetry helper on a 1→16s schedule; only 429/5xx are retryable; honors Retry-After.",
  },
  {
    n: 409, title: "feat(auth): rotating refresh token store", author: "ag_jwt", branch: "agent/jwt-refresh", base: "main",
    state: "open", draft: true, rel: "5h", additions: 159, deletions: 18, comments: 7, approvals: 0, reviewers: ["mara"],
    sha: "7c4d11a",
    checks: [{ name: "ci / typecheck", state: "pass", ms: 39000 }, { name: "ci / unit", state: "fail", ms: 88000 }, { name: "ci / e2e", state: "skipped", ms: 0 }],
    body: "Short-lived access JWTs backed by a Postgres-persisted rotating refresh store. Blocked on a decision: Redis or Postgres for the store.",
  },
  {
    n: 407, title: "test: checkout capture integration suite", author: "ag_checkout", branch: "agent/checkout-tests", base: "main",
    state: "merged", draft: false, rel: "2h", additions: 236, deletions: 4, comments: 1, approvals: 2, reviewers: ["mara", "devin"],
    sha: "9b21e07",
    checks: [{ name: "ci / typecheck", state: "pass", ms: 40000 }, { name: "ci / unit", state: "pass", ms: 102000 }, { name: "ci / e2e", state: "pass", ms: 402000 }],
    body: "Covers auth, confirm, capture, partial capture and decline paths.",
  },
  {
    n: 401, title: "fix(refund): banker's rounding on minor units", author: "mara", branch: "hotfix/refund-rounding", base: "main",
    state: "open", draft: false, rel: "yesterday", additions: 47, deletions: 9, comments: 2, approvals: 1, reviewers: ["devin"],
    sha: "e3b7d20",
    checks: [{ name: "ci / typecheck", state: "pass", ms: 38000 }, { name: "ci / unit", state: "pass", ms: 91000 }],
    body: "JPY partial captures rounded a cent high — switch to half-even rounding.",
  },
];

const CHECK_META = {
  pass: { icon: "check", color: "var(--st-done)" }, fail: { icon: "x", color: "var(--st-blocked)" },
  running: { icon: "refresh", color: "var(--accent-2)" }, skipped: { icon: "dots", color: "var(--ink-4)" },
};
const PR_STATE = { open: { color: "var(--st-done)", label: "open" }, merged: { color: "var(--accent)", label: "merged" }, closed: { color: "var(--st-blocked)", label: "closed" } };

function GithubPanel({ ctx }) {
  const prs = window.GITHUB_PRS;
  const [sel, setSel] = useStateGh(prs[0].n);
  const [filter, setFilter] = useStateGh("open");
  const [reviewed, setReviewed] = useStateGh({});
  const pr = prs.find((p) => p.n === sel) || prs[0];
  const shown = prs.filter((p) => filter === "all" || p.state === filter);

  // the PR's files come from the commit it points at — same data the diff views use
  const commit = ((window.GIT || {}).history || []).find((c) => c.sha === pr.sha);
  const files = (commit && commit.files) || [];
  const [selFile, setSelFile] = useStateGh(files[0] ? files[0].path : null);
  const diffKey = pr.sha + "/" + (selFile || "");
  const diff = ((window.GIT || {}).commitDiffs || {})[diffKey];

  const act = (kind) => {
    setReviewed((r) => ({ ...r, [pr.n]: kind }));
    ctx.flash(kind === "approve" ? "approved #" + pr.n : kind === "changes" ? "requested changes on #" + pr.n : "commented on #" + pr.n);
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "310px 1fr", minHeight: 0, flex: 1 }}>
      {/* PR list */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "7px 10px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <Icon name="pr" size="sm" style={{ color: "var(--accent)" }} />
          <span className="up" style={{ fontSize: 8.5, color: "var(--ink-3)" }}>{(ctx.project ? (ctx.project.org || "northwind") + "/" + ctx.project.name : "northwind/payments-service")}</span>
          <select className="osel" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ marginLeft: "auto", width: 92, padding: "3px 8px", fontSize: 10.5, flex: "none" }}>
            <option value="open">Open</option><option value="merged">Merged</option><option value="all">All</option>
          </select>
        </div>
        <div className="scroll-y" style={{ flex: 1 }}>
          {shown.map((p) => {
            const st = PR_STATE[p.state];
            const failing = p.checks.some((c) => c.state === "fail");
            return (
              <div key={p.n} onClick={() => { setSel(p.n); const f = (((window.GIT || {}).history || []).find((c) => c.sha === p.sha) || { files: [] }).files[0]; setSelFile(f ? f.path : null); }}
                style={{ padding: "8px 10px", cursor: "pointer", borderBottom: "1px solid var(--hair)", borderLeft: "2px solid " + (sel === p.n ? "var(--accent)" : "transparent"),
                  background: sel === p.n ? "var(--panel-3)" : "transparent" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <Icon name="pr" size="sm" style={{ width: 12, height: 12, color: st.color }} />
                  <span className="tnum" style={{ fontSize: 10, color: "var(--ink-4)" }}>#{p.n}</span>
                  {p.draft && <span className="chip" style={{ fontSize: 8.5, padding: "0 5px" }}>draft</span>}
                  <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{p.rel}</span>
                </div>
                <div style={{ fontSize: 11.5, color: sel === p.n ? "var(--ink)" : "var(--ink-2)", margin: "4px 0 5px", lineHeight: 1.4 }}>{p.title}</div>
                <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 9.5, color: "var(--ink-4)" }} className="tnum">
                  <span style={{ color: "var(--code-add-ink)" }}>+{p.additions}</span>
                  <span style={{ color: "var(--code-del-ink)" }}>−{p.deletions}</span>
                  <span style={{ display: "flex", gap: 3, alignItems: "center" }}><Icon name="chat" size="sm" style={{ width: 10, height: 10 }} />{p.comments}</span>
                  {p.approvals > 0 && <span style={{ display: "flex", gap: 3, alignItems: "center", color: "var(--st-done)" }}><Icon name="check" size="sm" style={{ width: 10, height: 10 }} />{p.approvals}</span>}
                  <span style={{ marginLeft: "auto", color: failing ? "var(--st-blocked)" : "var(--ink-4)" }}>{failing ? "checks failing" : p.checks.some((c) => c.state === "running") ? "checks running" : "checks green"}</span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* PR detail */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ padding: "9px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span className="chip" style={{ fontSize: 9, color: PR_STATE[pr.state].color, borderColor: "color-mix(in oklch, " + PR_STATE[pr.state].color + ", transparent 60%)" }}>
              <Icon name="pr" size="sm" style={{ width: 10, height: 10 }} />{PR_STATE[pr.state].label}
            </span>
            <span style={{ fontSize: 12.5, color: "var(--ink)" }}>{pr.title}</span>
            <span className="tnum" style={{ fontSize: 10, color: "var(--ink-4)" }}>#{pr.n}</span>
            <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
              <button className="btn ghost-hair" style={{ fontSize: 10.5, padding: "3px 8px" }} onClick={() => ctx.flash("cloned a worktree from " + pr.branch + " — spawn an agent to work in it")}>
                <Icon name="folderOpen" size="sm" />Open worktree
              </button>
              <button className="btn ghost-hair" style={{ fontSize: 10.5, padding: "3px 8px" }} onClick={() => ctx.flash("opened #" + pr.n + " on github.com")}>
                <Icon name="ext" size="sm" />GitHub
              </button>
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 7, fontSize: 10, color: "var(--ink-3)" }} className="tnum">
            {window.AuthorChip ? <window.AuthorChip id={pr.author} /> : <span>{pr.author}</span>}
            <span style={{ display: "flex", gap: 4, alignItems: "center" }}><Icon name="branch" size="sm" style={{ width: 11, height: 11, color: "var(--accent-2)" }} />{pr.branch} → {pr.base}</span>
            <span style={{ color: "var(--code-add-ink)" }}>+{pr.additions}</span>
            <span style={{ color: "var(--code-del-ink)" }}>−{pr.deletions}</span>
            <span>{files.length} files</span>
          </div>
          <div style={{ fontSize: 11, color: "var(--ink-2)", marginTop: 7, lineHeight: 1.5 }}>{pr.body}</div>
          {/* checks */}
          <div style={{ display: "flex", gap: 6, marginTop: 8, flexWrap: "wrap" }}>
            {pr.checks.map((c) => {
              const m = CHECK_META[c.state];
              return (
                <span key={c.name} className="chip" style={{ fontSize: 9.5, color: m.color, borderColor: "color-mix(in oklch, " + m.color + ", transparent 66%)" }}>
                  <Icon name={m.icon} size="sm" style={{ width: 10, height: 10 }} />{c.name}
                  {c.ms > 0 && <span style={{ color: "var(--ink-4)" }}>{Math.round(c.ms / 1000)}s</span>}
                </span>
              );
            })}
          </div>
          {/* review actions */}
          <div style={{ display: "flex", alignItems: "center", gap: 7, marginTop: 9 }}>
            <button className="btn primary" style={{ fontSize: 11 }} disabled={reviewed[pr.n] === "approve"} onClick={() => act("approve")}>
              <Icon name="check" size="sm" />{reviewed[pr.n] === "approve" ? "Approved" : "Approve"}
            </button>
            <button className="btn ghost-hair" style={{ fontSize: 11, color: "var(--st-blocked)" }} onClick={() => act("changes")}><Icon name="flag" size="sm" />Request changes</button>
            <button className="btn ghost-hair" style={{ fontSize: 11 }} onClick={() => act("comment")}><Icon name="chat" size="sm" />Comment</button>
            <span style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>hover a line in the diff to leave an inline comment</span>
          </div>
        </div>

        {/* files + diff — same review surface as agent diffs */}
        <div style={{ flex: 1, display: "grid", gridTemplateColumns: "228px 1fr", minHeight: 0 }}>
          <div className="scroll-y" style={{ borderRight: "1px solid var(--hair)", padding: "4px 0", background: "var(--panel)" }}>
            {files.map((f) => (
              <div key={f.path} onClick={() => setSelFile(f.path)}
                style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 10px", cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)",
                  background: selFile === f.path ? "var(--panel-3)" : "transparent" }}>
                <Icon name="file" size="sm" style={{ width: 12, height: 12, color: f.state === "A" ? "var(--code-add-ink)" : "var(--accent-2)" }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 11, color: selFile === f.path ? "var(--ink)" : "var(--ink-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileName(f.path)}</div>
                  <div style={{ fontSize: 9, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</div>
                </div>
                <span className="tnum" style={{ fontSize: 9, color: "var(--code-add-ink)", flex: "none" }}>+{f.add}</span>
              </div>
            ))}
          </div>
          {diff
            ? <ReviewCode agentId={"pr-" + pr.n} file={selFile} view="diff" rows={diffToRows({ ...diff, file: selFile })} lang={diff.lang} />
            : <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)", fontSize: 11.5, padding: 20, textAlign: "center" }}>
                {selFile ? "diff for " + fileName(selFile) + " is fetched on demand from the GitHub API" : "select a file"}
              </div>}
        </div>
      </div>
    </div>
  );
}

window.TOOL_PANELS.github = { kind: "github", label: "Pull Requests", icon: "pr", Component: GithubPanel };
Object.assign(window, { GithubPanel });
