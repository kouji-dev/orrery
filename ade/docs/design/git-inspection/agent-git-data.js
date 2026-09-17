/* global window */
// Orrery — agent-scoped git inspection data builders.
// Derives per-agent branch history, blame, file history and conflict sessions
// from the existing AGENTS / COMMITS / DIFFS, reusing window.GIT (authors,
// conflict template) registered by repo-data.js.

(function () {
  const G = window.GIT;
  const AGENTS = window.AGENTS || [];
  const COMMITS = window.COMMITS || [];
  const DIFFS = window.DIFFS || {};
  const TOOLS = window.AGENT_TOOLS || [];
  const glyph = { claude: "✳", codex: "◆", cursor: "▸", gemini: "✦" };

  // register each agent as a commit author (tool-colored)
  AGENTS.forEach((ag) => {
    const tm = TOOLS.find((t) => t.id === ag.tool) || {};
    G.authors["self_" + ag.id] = { name: ag.name, short: glyph[ag.tool] || "A", color: tm.accent || "var(--accent)", kind: "agent", tool: ag.tool };
  });

  // reuse each agent's rich primary-file diff under its head sha
  AGENTS.forEach((ag) => {
    const d = DIFFS[ag.id];
    const c = COMMITS.find((x) => x.agent === ag.id);
    if (d && c) G.commitDiffs[c.sha + "/" + d.file] = { lang: d.lang, hunks: d.hunks };
  });

  const shaFrom = (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0; return h.toString(16).padStart(7, "0").slice(0, 7); };
  const baseAuthors = ["mara", "devin", "priya", "tom"];
  const relForBase = { mara: "5d", devin: "1w", priya: "3d", tom: "4d" };

  // ---- branch commit history (newest → oldest) ----
  function agentHistory(ag) {
    const self = "self_" + ag.id;
    const headCommit = COMMITS.find((x) => x.agent === ag.id);
    const files = ag.files && ag.files.length ? ag.files : [{ path: "src/" + ag.name.replace(/[^a-z0-9]/gi, "-") + ".ts", state: "M", add: 8, del: 2 }];
    const out = [];
    if (headCommit) {
      out.push({ sha: headCommit.sha, author: self, rel: headCommit.when, date: "today", msg: headCommit.msg, files, head: true });
    }
    // earlier scaffold commits derived from the task
    const n = Math.max(1, Math.min(3, ag.commits || 1));
    const subjects = [
      "wip: " + ag.name + " — implement core path",
      "test: cover " + ag.name + " edge cases",
      "chore: branch from " + ag.base,
    ];
    for (let i = 0; i < n; i++) {
      const subFiles = files.slice(0, Math.max(1, files.length - 1 - i));
      out.push({ sha: shaFrom(ag.id + "c" + i), author: self, rel: (i + 2) + "h", date: "today", msg: subjects[i % subjects.length], files: subFiles.length ? subFiles : files.slice(0, 1) });
    }
    return out;
  }

  // ---- per-line blame for an agent file ----
  function agentBlame(ag, path) {
    const lines = (window.getFileLines ? window.getFileLines(path, ag) : []).slice(0, 40);
    const headCommit = COMMITS.find((x) => x.agent === ag.id);
    const headSha = headCommit ? headCommit.sha : ag.base;
    const ages = [1.0, 0.62, 0.42];
    return lines.map((ln, i) => {
      if (ln.add) {
        return { n: ln.n, s: ln.s, sha: headSha, author: "self_" + ag.id, rel: "now", age: 0.05 };
      }
      const a = baseAuthors[(i + path.length) % baseAuthors.length];
      return { n: ln.n, s: ln.s, sha: ag.base, author: a, rel: relForBase[a], age: ages[i % ages.length] };
    });
  }

  // ---- file history (revisions touching a file) ----
  function agentFileHistory(ag, path) {
    const d = DIFFS[ag.id];
    const headCommit = COMMITS.find((x) => x.agent === ag.id);
    const cf = (ag.files || []).find((f) => f.path === path) || { add: 8, del: 3 };
    const fn = (window.fileName || ((p) => p))(path);
    const revisions = [
      { sha: headCommit ? headCommit.sha : shaFrom(ag.id + path), author: "self_" + ag.id, rel: headCommit ? headCommit.when : "now", date: "today", msg: headCommit ? headCommit.msg : "edit " + fn, add: cf.add, del: cf.del },
      { sha: ag.base, author: "mara", rel: "5d", date: "Jun 11", msg: "feat: earlier work on " + fn, add: 22, del: 6 },
      { sha: shaFrom("scaffold" + path), author: "devin", rel: "1w", date: "Jun 09", msg: "chore: scaffold " + fn, add: 14, del: 0 },
    ];
    const diff = d && d.file === path ? { lang: d.lang, hunks: d.hunks } : (window.synthDiff ? window.synthDiff({ path, state: "M", add: cf.add, del: cf.del }) : { lang: "text", hunks: [] });
    return { path, lang: (window.langOf || (() => "text"))(path), revisions, diff };
  }

  // ---- which agents currently have an in-progress conflicted merge ----
  const CONFLICT_AGENTS = ["b2"]; // jwt-refresh (auth/jwt.ts) — matches the conflict template
  function agentHasConflict(ag) { return CONFLICT_AGENTS.includes(ag.id); }

  function agentConflict(ag, project) {
    const base = G.conflict;
    const target = project ? project.branch : "main";
    const mine = ag.branch.replace("agent/", "");
    return Object.assign({}, base, {
      op: "merge",
      title: "Merge " + target + " → " + mine,
      ours: mine,
      theirs: target,
    });
  }

  window.AGENT_GIT = { agentHistory, agentBlame, agentFileHistory, agentHasConflict, agentConflict };
})();
