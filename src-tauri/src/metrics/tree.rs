//! Recursive process tree for the perf panel (A7.7): the app's pid and each
//! agent's PTY child as roots, per-node cpu / private bytes / RSS, and rolled-up
//! subtree totals.
//!
//! Two correctness rules (from the roadmap) shape this module:
//! 1. **Parent-pid walking is not trusted alone.** WebView2 children reparent
//!    and agent grandchildren detach. The kill-on-close Job Object every one of
//!    our descendants inherits ([`crate::runtime::jobobj::JobGuard`]) is the
//!    discovery backstop: any job member the walk missed is surfaced as a
//!    "detached" child of the app root instead of silently vanishing.
//! 2. **Subtree totals never sum RSS.** RSS double-counts shared pages (badly,
//!    with many similar WebView2/node processes). Totals sum PRIVATE bytes;
//!    RSS stays a per-node secondary column.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

/// One process's raw sample for the tree builder. Kept sysinfo-free so the
/// builder is unit-testable from synthetic maps (same pattern as ProcSample).
#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub parent: Option<u32>,
    pub cpu: f32,
    /// Resident set (sysinfo `memory()`) — the SECONDARY column.
    pub rss: u64,
    pub name: String,
}

/// One node in the rendered tree. camelCase for the frontend.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessNode {
    pub pid: u32,
    pub name: String,
    /// Self-explaining annotation for known kinds ("webview2 · not our code",
    /// "conhost · ConPTY host", "pwsh · shim wrapper", …). None = unannotated.
    pub note: Option<String>,
    /// Machine-relative % (already ÷ cores, like Task Manager).
    pub cpu: f32,
    /// Private bytes — the HEADLINE memory number (no shared-page double count).
    pub priv_bytes: u64,
    /// Resident set — secondary; never rolled up.
    pub rss_bytes: u64,
    /// Rolled-up subtree totals (this node + every descendant).
    pub subtree_cpu: f32,
    pub subtree_priv_bytes: u64,
    /// Process count in the subtree (this node included).
    pub subtree_procs: u32,
    /// In the Job Object but unreachable via the parent-pid walk — surfaced
    /// under the app root so nothing we spawned can hide.
    pub detached: bool,
    /// An external browser Orrery merely LAUNCHED (link handoff): rendered,
    /// but its subtree is excluded from every parent rollup.
    pub excluded: bool,
    pub children: Vec<ProcessNode>,
}

/// One root: the app ("app"/"Orrery") or an agent (uuid / display name).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessTreeRoot {
    pub id: String,
    pub label: String,
    pub node: ProcessNode,
}

/// The whole snapshot returned by the `process_tree` command (pull-based:
/// the frontend polls only while the perf panel is open — the A1.7 gating
/// pattern — so no background cost exists when nobody is looking).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessTreeSnapshot {
    pub roots: Vec<ProcessTreeRoot>,
    pub ts_ms: u64,
}

/// Lowercased basename without `.exe`, for kind matching.
fn base_name(name: &str) -> String {
    let base = name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

/// External browsers Orrery may LAUNCH (opening a link hands off to the OS
/// default browser — if it wasn't already running, its entire process tree
/// gets parented under orrery.exe). Their lifecycle and memory are not ours:
/// they are rendered in the tree (nothing hides) but EXCLUDED from subtree
/// rollups, so "Orrery 1.2GB" can never mean "Opera is open".
/// `msedgewebview2` must NOT match — that IS our UI's memory.
fn is_external_browser(name: &str) -> bool {
    matches!(
        base_name(name).as_str(),
        "opera" | "opera_gx" | "chrome" | "msedge" | "firefox" | "brave" | "vivaldi" | "arc"
            | "librewolf" | "waterfox" | "iron" | "thorium"
    )
}

/// Annotate known process kinds so the panel is self-explaining.
/// Matching is on the lowercased basename without `.exe`. `excluded_browser`
/// distinguishes a link-handoff browser (excluded from totals) from the same
/// binary launched by an AGENT (MCP automation — counted as its workload).
fn annotate(name: &str, is_app_root: bool, excluded_browser: bool) -> Option<String> {
    if is_app_root {
        return Some("rust core · our code".into());
    }
    if excluded_browser {
        return Some("external browser · opened via link — excluded from totals".into());
    }
    if is_external_browser(name) {
        return Some("browser · agent workload (counted)".into());
    }
    let note = match base_name(name).as_str() {
        // Orrery == the Tauri runtime: the WebView2 family renders our UI and
        // is fully part of Orrery's own footprint — displayed and counted.
        "msedgewebview2" => "webview2 · Orrery UI runtime",
        "conhost" | "openconsole" => "conhost · ConPTY host",
        "pwsh" | "powershell" => "pwsh · shim wrapper (see A0.1)",
        "cmd" => "cmd · shim wrapper (see A0.1)",
        _ => return None,
    };
    Some(note.into())
}

/// Build the parent→children adjacency once per snapshot.
fn adjacency(map: &HashMap<u32, ProcInfo>) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, s) in map {
        if let Some(parent) = s.parent {
            children.entry(parent).or_default().push(*pid);
        }
    }
    children
}

/// Recursively build the node for `pid`. `stop` pids (the other roots) are
/// excluded so an agent subtree is never double-counted inside the app root.
/// `visited` guards pid-reuse cycles and feeds the job-object backstop.
///
/// `cut_browser`: this node, if an external browser, is a LINK HANDOFF and is
/// excluded from rollups. True only for direct children of the app root (and
/// app-root job strays) — a browser inside an AGENT's subtree is that agent's
/// workload (MCP browser automation, Playwright runs) and always counts.
fn build_node(
    pid: u32,
    map: &HashMap<u32, ProcInfo>,
    children: &HashMap<u32, Vec<u32>>,
    stop: &HashSet<u32>,
    visited: &mut HashSet<u32>,
    priv_of: &dyn Fn(u32, &ProcInfo) -> u64,
    is_app_root: bool,
    cut_browser: bool,
) -> ProcessNode {
    visited.insert(pid);
    let info = map.get(&pid);
    let (name, cpu, rss) = match info {
        Some(i) => (i.name.clone(), i.cpu, i.rss),
        None => ("<gone>".into(), 0.0, 0), // died between refresh and build
    };
    let priv_bytes = info.map(|i| priv_of(pid, i)).unwrap_or(0);
    // Plain loop (not iterator chain): `visited` must be re-checked per child
    // because an earlier sibling's recursion may have claimed a shared pid.
    let mut kids: Vec<ProcessNode> = Vec::new();
    if let Some(c) = children.get(&pid) {
        for k in c {
            if stop.contains(k) || visited.contains(k) {
                continue;
            }
            // only the APP ROOT's direct children are link-handoff candidates
            kids.push(build_node(*k, map, children, stop, visited, priv_of, false, is_app_root));
        }
    }
    // Stable order: biggest subtree first — that's what the eye hunts for.
    kids.sort_by(|a, b| b.subtree_priv_bytes.cmp(&a.subtree_priv_bytes));
    // Excluded (external-browser) children keep their OWN subtree numbers for
    // display, but contribute nothing to this node's rollup — their memory is
    // not Orrery's story even though the ppid chain is real.
    let counted = kids.iter().filter(|k| !k.excluded);
    let subtree_cpu = cpu + counted.clone().map(|k| k.subtree_cpu).sum::<f32>();
    let subtree_priv = priv_bytes + counted.clone().map(|k| k.subtree_priv_bytes).sum::<u64>();
    let subtree_procs = 1 + counted.map(|k| k.subtree_procs).sum::<u32>();
    let excluded = cut_browser && is_external_browser(&name);
    ProcessNode {
        pid,
        note: annotate(&name, is_app_root, excluded),
        excluded,
        name,
        cpu,
        priv_bytes,
        rss_bytes: rss,
        subtree_cpu,
        subtree_priv_bytes: subtree_priv,
        subtree_procs,
        detached: false,
        children: kids,
    }
}

/// Build the whole snapshot. `agents` is `(id, label, pid)` per running agent;
/// `job_pids` is the Job Object membership (the discovery backstop) — `None`
/// when unavailable (non-Windows / job install failed). `webview_roots` are
/// OUR WebView2 browser-process pids (from `BrowserProcessId`): attached under
/// the app root even when the ppid walk can't reach them — WebView2 shares one
/// browser family per user-data-dir, so a second Orrery instance's UI parents
/// under the first.
pub fn build_tree(
    app_pid: u32,
    agents: &[(String, String, u32)],
    map: &HashMap<u32, ProcInfo>,
    job_pids: Option<&[u32]>,
    webview_roots: &[u32],
    priv_of: &dyn Fn(u32, &ProcInfo) -> u64,
    now_ms: u64,
) -> ProcessTreeSnapshot {
    let children = adjacency(map);
    // Agent roots are carved OUT of the app walk: each is its own top-level
    // tree, so the status-bar split (Orrery vs agents) is a plain subtraction.
    let stop: HashSet<u32> = agents.iter().map(|(_, _, pid)| *pid).collect();
    let mut visited: HashSet<u32> = HashSet::new();

    let mut app_node = build_node(
        app_pid,
        map,
        &children,
        &stop,
        &mut visited,
        priv_of,
        true,
        false, // the app root itself is never a browser
    );

    let agent_roots: Vec<ProcessTreeRoot> = agents
        .iter()
        .map(|(id, label, pid)| ProcessTreeRoot {
            id: id.clone(),
            label: label.clone(),
            node: build_node(
                *pid,
                map,
                &children,
                &HashSet::new(), // an agent's own descendants are never carved out
                &mut visited,
                priv_of,
                false,
                false, // agent-launched browsers (MCP automation) always COUNT
            ),
        })
        .collect();

    // Our WebView2 family, attached explicitly when the ppid walk missed it
    // (its browser process parents under another Orrery instance when one was
    // already running — shared browser family per user-data-dir). Counted:
    // this IS Orrery's UI runtime, whatever pid it hangs off.
    for pid in webview_roots {
        if visited.contains(pid) || !map.contains_key(pid) {
            continue; // ppid walk already reached it — nothing to repair
        }
        let n = build_node(*pid, map, &children, &stop, &mut visited, priv_of, false, false);
        app_node.subtree_cpu += n.subtree_cpu;
        app_node.subtree_priv_bytes += n.subtree_priv_bytes;
        app_node.subtree_procs += n.subtree_procs;
        app_node.children.push(n);
    }

    // Job-object backstop: any job member the parent walk never reached gets
    // pinned (flat) under the app root, flagged `detached`. Its own subtree is
    // still walked so a detached `next dev` shows its children too.
    if let Some(job) = job_pids {
        let strays: Vec<u32> = job
            .iter()
            .filter(|p| !visited.contains(p) && map.contains_key(p))
            .copied()
            .collect();
        for pid in strays {
            if visited.contains(&pid) {
                continue; // an earlier stray's walk may have covered this one
            }
            // cut_browser=true: browsers commonly detach from their launcher, so
            // a link-handoff Opera whose intermediate exited lands here — it must
            // stay excluded. (Tradeoff: an AGENT's broken-chain browser stray is
            // also dimmed rather than counted — the lesser misattribution.)
            let mut n = build_node(pid, map, &children, &stop, &mut visited, priv_of, false, true);
            n.detached = true;
            if !n.excluded {
                app_node.subtree_cpu += n.subtree_cpu;
                app_node.subtree_priv_bytes += n.subtree_priv_bytes;
                app_node.subtree_procs += n.subtree_procs;
            }
            app_node.children.push(n);
        }
    }

    let mut roots = Vec::with_capacity(agent_roots.len() + 1);
    roots.push(ProcessTreeRoot {
        id: "app".into(),
        label: "Orrery".into(),
        node: app_node,
    });
    roots.extend(agent_roots);
    ProcessTreeSnapshot { roots, ts_ms: now_ms }
}

// ------------------------------------------------- platform memory shim
// sysinfo's `memory()` is a resident-set number on every platform, which
// double-counts shared pages when summed. The headline needs a private
// figure: private commit (PrivateUsage) on Windows; elsewhere we fall back
// to RSS until the PSS (Linux smaps_rollup) / phys_footprint (macOS) shims
// land with the Phase-0 ports.

/// Private bytes for `pid`, falling back to the sample's RSS when the platform
/// query fails (access denied, process gone).
pub fn private_bytes_or_rss(pid: u32, info: &ProcInfo) -> u64 {
    #[cfg(windows)]
    {
        if let Some(p) = imp::private_bytes(pid) {
            return p;
        }
    }
    let _ = pid;
    info.rss
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// PrivateUsage (private commit) via psapi. One OpenProcess per pid per
    /// snapshot — the pid set is tens of processes and the poll only runs
    /// while the perf panel is open, so this stays far below sampling budget.
    pub fn private_bytes(pid: u32) -> Option<u64> {
        // SAFETY: standard Win32 FFI — handle checked, zeroed correctly-sized
        // struct, handle closed on every path.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            let ok = K32GetProcessMemoryInfo(
                h,
                &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            );
            CloseHandle(h);
            (ok != 0).then_some(counters.PrivateUsage as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(parent: Option<u32>, cpu: f32, rss: u64, name: &str) -> ProcInfo {
        ProcInfo {
            parent,
            cpu,
            rss,
            name: name.into(),
        }
    }

    /// Synthetic tree:
    ///   1 orrery ── 2 msedgewebview2 ── 3 msedgewebview2(renderer)
    ///           └── 10 conhost ── 11 node (agent) ── 12 next dev
    ///   99 stray-in-job (no parent chain to 1)
    fn fixture() -> HashMap<u32, ProcInfo> {
        let mut m = HashMap::new();
        m.insert(1, info(None, 1.0, 100, "orrery.exe"));
        m.insert(2, info(Some(1), 2.0, 200, "msedgewebview2.exe"));
        m.insert(3, info(Some(2), 3.0, 300, "msedgewebview2.exe"));
        m.insert(10, info(Some(1), 0.5, 50, "conhost.exe"));
        m.insert(11, info(Some(10), 10.0, 1000, "node.exe"));
        m.insert(12, info(Some(11), 5.0, 500, "node.exe"));
        m.insert(99, info(None, 7.0, 700, "vitest.exe"));
        m
    }

    fn rss_priv(_pid: u32, i: &ProcInfo) -> u64 {
        i.rss
    }

    #[test]
    fn agent_roots_are_carved_out_of_the_app_walk() {
        let m = fixture();
        // conhost (10) is the agent PTY child root.
        let snap = build_tree(
            1,
            &[("ag".into(), "nova".into(), 10)],
            &m,
            None,
            &[], &rss_priv,
            0,
        );
        assert_eq!(snap.roots.len(), 2);
        let app = &snap.roots[0].node;
        // app subtree = 1 + 2 + 3 only (agent branch carved out)
        assert_eq!(app.subtree_priv_bytes, 100 + 200 + 300);
        assert_eq!(app.subtree_procs, 3);
        let ag = &snap.roots[1].node;
        assert_eq!(ag.subtree_priv_bytes, 50 + 1000 + 500);
        assert_eq!(ag.subtree_cpu, 0.5 + 10.0 + 5.0);
        assert_eq!(snap.roots[1].label, "nova");
    }

    #[test]
    fn job_strays_attach_detached_under_the_app_root() {
        let m = fixture();
        let job = vec![1, 2, 3, 10, 11, 12, 99];
        let snap = build_tree(
            1,
            &[("ag".into(), "nova".into(), 10)],
            &m,
            Some(&job),
            &[],
            &rss_priv,
            0,
        );
        let app = &snap.roots[0].node;
        let stray = app
            .children
            .iter()
            .find(|c| c.pid == 99)
            .expect("stray surfaced");
        assert!(stray.detached);
        assert_eq!(app.subtree_priv_bytes, 100 + 200 + 300 + 700);
    }

    #[test]
    fn known_kinds_are_annotated() {
        assert_eq!(
            annotate("msedgewebview2.exe", false, false).as_deref(),
            Some("webview2 · Orrery UI runtime")
        );
        assert_eq!(
            annotate("conhost.exe", false, false).as_deref(),
            Some("conhost · ConPTY host")
        );
        assert!(annotate("pwsh.exe", false, false).unwrap().contains("shim wrapper"));
        assert_eq!(annotate("node.exe", false, false), None);
        assert_eq!(
            annotate("orrery.exe", true, false).as_deref(),
            Some("rust core · our code")
        );
        // same binary, two stories: link handoff vs agent workload
        assert!(annotate("chrome.exe", false, true).unwrap().contains("excluded"));
        assert!(annotate("chrome.exe", false, false).unwrap().contains("agent workload"));
    }

    /// Diagnostic probe against the REAL machine: does a full sysinfo sweep
    /// populate `parent()` for WebView2 families, and does `build_tree` then
    /// see their children? Run manually:
    /// `cargo test real_system_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_system_probe() {
        let mut sampler = crate::metrics::MetricsSampler::new();
        sampler.refresh();
        let map = sampler.probe_proc_infos();
        let mut with_parent = 0;
        let mut without = 0;
        for (pid, info) in &map {
            if info.name.to_ascii_lowercase().contains("msedgewebview2") {
                match info.parent {
                    Some(pp) => {
                        with_parent += 1;
                        println!("webview {pid} parent={pp} rss={}MB", info.rss / 1_048_576);
                    }
                    None => {
                        without += 1;
                        println!("webview {pid} parent=NONE rss={}MB", info.rss / 1_048_576);
                    }
                }
            }
        }
        println!("webviews: {with_parent} with parent, {without} without");
        // pick any webview browser process (one that other webviews parent to)
        // and check build_tree finds its children
        let browsers: Vec<u32> = map
            .iter()
            .filter(|(_, i)| i.name.to_ascii_lowercase().contains("msedgewebview2"))
            .filter(|(pid, _)| {
                map.values().any(|o| o.parent == Some(**pid))
            })
            .map(|(pid, _)| *pid)
            .collect();
        for b in browsers {
            let snap = build_tree(b, &[], &map, None, &[], &rss_priv, 0);
            let n = &snap.roots[0].node;
            println!(
                "root {b}: {} direct children, subtree {} procs, {}MB",
                n.children.len(),
                n.subtree_procs,
                n.subtree_priv_bytes / 1_048_576
            );
        }
    }

    /// Does a scoped refresh (`ProcessesToUpdate::Some` + remove_dead=true)
    /// evict unlisted processes from sysinfo's map?
    /// `cargo test scoped_refresh_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scoped_refresh_probe() {
        let mut sampler = crate::metrics::MetricsSampler::new();
        sampler.refresh();
        let full = sampler.probe_proc_infos().len();
        let me = std::process::id();
        sampler.refresh_scoped(&[me]);
        let after = sampler.probe_proc_infos().len();
        println!("map size: full sweep = {full}, after scoped refresh on self = {after}");
    }

    /// Does a scoped refresh of an ALREADY-KNOWN process wipe its `parent()`?
    /// `cargo test parent_persistence_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn parent_persistence_probe() {
        let mut sampler = crate::metrics::MetricsSampler::new();
        sampler.refresh();
        let before = sampler.probe_proc_infos();
        let webviews: Vec<(u32, Option<u32>)> = before
            .iter()
            .filter(|(_, i)| i.name.to_ascii_lowercase().contains("msedgewebview2"))
            .map(|(pid, i)| (*pid, i.parent))
            .collect();
        println!("before: {} webviews, all parents known", webviews.len());
        // scoped refresh over exactly those pids (what the app does per tick)
        let pids: Vec<u32> = webviews.iter().map(|(p, _)| *p).collect();
        sampler.refresh_scoped(&pids);
        let after = sampler.probe_proc_infos();
        let mut wiped = 0;
        for (pid, was) in &webviews {
            let now = after.get(pid).and_then(|i| i.parent);
            if now != *was {
                wiped += 1;
                println!("pid {pid}: parent {was:?} -> {now:?}");
            }
        }
        println!("parents changed/wiped after scoped refresh: {wiped}/{}", webviews.len());
    }

    /// Full lifecycle simulation of the live app: warm-up sweep BEFORE the
    /// children exist (webviews spawn after), then scoped polls only — how many
    /// polls until the children are discovered?
    /// `cargo test discovery_lifecycle_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn discovery_lifecycle_probe() {
        let me = std::process::id();
        let mut sampler = crate::metrics::MetricsSampler::new();
        sampler.refresh(); // warm_up: children don't exist yet
        let mut kids: Vec<std::process::Child> = (0..3)
            .map(|_| {
                std::process::Command::new("ping")
                    .args(["-n", "60", "127.0.0.1"])
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(400));
        for i in 1..=12 {
            sampler.refresh_scoped(&[me]);
            let map = sampler.probe_proc_infos();
            let snap = build_tree(me, &[], &map, None, &[], &rss_priv, 0);
            println!(
                "poll {i}: root children = {}, subtree procs = {}",
                snap.roots[0].node.children.len(),
                snap.roots[0].node.subtree_procs
            );
        }
        for k in &mut kids {
            let _ = k.kill();
        }
    }

    // Orrery launches the default browser on link clicks; if it wasn't already
    // running, its whole tree parents under orrery.exe. It must be RENDERED
    // (nothing hides) but excluded from every rollup — "Orrery 1.2GB" must
    // never mean "Opera is open".
    #[test]
    fn external_browser_is_rendered_but_excluded_from_totals() {
        let mut m = fixture();
        m.insert(50, info(Some(1), 4.0, 400, "opera.exe"));
        m.insert(51, info(Some(50), 6.0, 600, "opera.exe"));
        let snap = build_tree(1, &[], &m, None, &[], &rss_priv, 0);
        let app = &snap.roots[0].node;
        let opera = app
            .children
            .iter()
            .find(|c| c.pid == 50)
            .expect("browser rendered");
        assert!(opera.excluded);
        assert!(opera.note.as_deref().unwrap_or("").contains("external browser"));
        // its own subtree numbers exist for display…
        assert_eq!(opera.subtree_priv_bytes, 400 + 600);
        // …but the app rollup does not include them (fixture app tree = 100+200+300
        // + the conhost/node branch 50+1000+500 — no agent carve in this test)
        assert_eq!(app.subtree_priv_bytes, 100 + 200 + 300 + 50 + 1000 + 500);
        assert!(!app.children.iter().any(|c| c.pid == 50 && !c.excluded));
        // webview2 is NOT a browser in this sense — it stays counted
        assert!(!is_external_browser("msedgewebview2.exe"));
        assert!(is_external_browser("opera.exe"));
        assert!(is_external_browser(r"C:\x\chrome.exe"));
    }

    // A browser INSIDE an agent's subtree (MCP browser automation, Playwright)
    // is that agent's workload: counted in its rollup, never dimmed.
    #[test]
    fn agent_launched_browser_counts_in_the_agent_subtree() {
        let mut m = fixture();
        m.insert(60, info(Some(11), 5.0, 500, "chrome.exe")); // under node(11) ← conhost(10)
        let snap = build_tree(
            1,
            &[("ag".into(), "nova".into(), 10)],
            &m,
            None,
            &[], &rss_priv,
            0,
        );
        let ag = &snap.roots[1].node;
        // agent subtree = conhost(50) + node(1000) + next(500) + chrome(500)
        assert_eq!(ag.subtree_priv_bytes, 50 + 1000 + 500 + 500);
        let node11 = ag.children.iter().find(|c| c.pid == 11).unwrap();
        let chrome = node11.children.iter().find(|c| c.pid == 60).unwrap();
        assert!(!chrome.excluded);
        assert!(chrome.note.as_deref().unwrap_or("").contains("agent workload"));
    }

    // WebView2 shares one browser family per user-data-dir: with another
    // Orrery instance already running, OUR UI processes parent under IT and
    // the ppid walk finds nothing. The BrowserProcessId-seeded root repairs
    // that: the family attaches under our app root, counted (it IS our UI).
    #[test]
    fn webview_family_attaches_when_ppid_points_at_another_instance() {
        let mut m = fixture();
        m.insert(90, info(None, 0.5, 80, "orrery.exe")); // the OTHER instance
        m.insert(91, info(Some(90), 2.0, 200, "msedgewebview2.exe")); // shared browser proc
        m.insert(92, info(Some(91), 3.0, 300, "msedgewebview2.exe"));
        let snap = build_tree(1, &[], &m, None, &[91], &rss_priv, 0);
        let app = &snap.roots[0].node;
        let wv = app
            .children
            .iter()
            .find(|c| c.pid == 91)
            .expect("webview family attached despite foreign ppid");
        assert!(!wv.excluded);
        assert!(wv.note.as_deref().unwrap_or("").contains("UI runtime"));
        assert_eq!(wv.subtree_priv_bytes, 200 + 300);
        // counted into the app rollup (fixture app tree 100+200+300+50+1000+500)
        assert_eq!(
            app.subtree_priv_bytes,
            100 + 200 + 300 + 50 + 1000 + 500 + 200 + 300
        );
        // already-reachable webview roots are NOT double-attached
        let snap2 = build_tree(1, &[], &m, None, &[2], &rss_priv, 0);
        let app2 = &snap2.roots[0].node;
        assert_eq!(app2.children.iter().filter(|c| c.pid == 2).count(), 1);
    }

    #[test]
    fn cycles_terminate() {
        let mut m = HashMap::new();
        m.insert(1, info(Some(2), 1.0, 10, "a"));
        m.insert(2, info(Some(1), 2.0, 20, "b"));
        let snap = build_tree(1, &[], &m, None, &[], &rss_priv, 0);
        assert_eq!(snap.roots[0].node.subtree_procs, 2);
    }
}
