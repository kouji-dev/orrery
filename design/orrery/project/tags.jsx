/* global React, Icon */
// ── Tags — snake_case labels: chips, search-or-create picker, filter dropdown ──
const { useState: useStateTG, useRef: useRefTG, useEffect: useEffectTG } = React;

// normalize any free text to a snake_case tag name
function snakeTag(s) {
  return (s || "").toLowerCase().trim().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 32);
}

// derive the sorted union of every tag used across a ticket list
function allTagsOf(tickets) {
  const set = new Set();
  (tickets || []).forEach((t) => (t.tags || []).forEach((tag) => set.add(tag)));
  return [...set].sort();
}

// ── a single tag chip (display / toggle / removable) ─────────────────────────
function Tag({ name, active, onClick, onRemove, dim }) {
  const clickable = !!onClick;
  const [h, setH] = useStateTG(false);
  return (
    <span onClick={clickable ? (e) => { e.stopPropagation(); onClick(name); } : undefined}
      onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)} title={name}
      style={{ display: "inline-flex", alignItems: "center", gap: 4, flex: "none", maxWidth: 170,
        fontFamily: "var(--font-mono)", fontSize: 10.5, lineHeight: 1, letterSpacing: "0.01em",
        padding: onRemove ? "3px 4px 3px 8px" : "3px 8px", borderRadius: 999, whiteSpace: "nowrap",
        cursor: clickable ? "pointer" : "default", userSelect: "none", transition: "background .12s, border-color .12s, color .12s",
        color: active ? "var(--accent)" : (dim ? "var(--ink-3)" : "var(--ink-2)"),
        border: "1px solid " + (active ? "color-mix(in oklch, var(--accent), transparent 52%)" : (h && clickable ? "var(--hair-2)" : "var(--hair)")),
        background: active ? "color-mix(in oklch, var(--accent), transparent 88%)" : (h && clickable ? "var(--panel-3)" : "var(--panel-2)") }}>
      <Icon name="tag" size="sm" style={{ width: 10, height: 10, flex: "none", opacity: 0.65 }} />
      <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{name}</span>
      {onRemove && (
        <button onClick={(e) => { e.stopPropagation(); onRemove(name); }} aria-label={"Remove " + name}
          style={{ display: "grid", placeItems: "center", border: "none", background: "transparent", cursor: "pointer",
            color: "var(--ink-4)", padding: 0, width: 14, height: 14, borderRadius: 4, flex: "none" }}
          onMouseEnter={(e) => e.currentTarget.style.color = "var(--ink)"} onMouseLeave={(e) => e.currentTarget.style.color = "var(--ink-4)"}>
          <Icon name="x" size="sm" style={{ width: 10, height: 10 }} />
        </button>
      )}
    </span>
  );
}

// ── floating popover: search existing tags or create a new one ───────────────
function TagPicker({ allTags, attached, onChange, onClose, align = "left" }) {
  const [q, setQ] = useStateTG("");
  const ref = useRefTG(null);
  const inputRef = useRefTG(null);

  useEffectTG(() => { if (inputRef.current) inputRef.current.focus(); }, []);
  useEffectTG(() => {
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) onClose(); };
    const onKey = (e) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("mousedown", onDoc); document.addEventListener("keydown", onKey);
    return () => { document.removeEventListener("mousedown", onDoc); document.removeEventListener("keydown", onKey); };
  }, [onClose]);

  const norm = snakeTag(q);
  const available = (allTags || []).filter((t) => !attached.includes(t));
  const matches = norm ? available.filter((t) => t.includes(norm)) : available;
  const exact = (allTags || []).includes(norm) || attached.includes(norm);
  const canCreate = !!norm && !exact;

  const attach = (t) => {
    if (!t || attached.includes(t)) return;
    onChange([...attached, t]); setQ("");
    if (inputRef.current) inputRef.current.focus();
  };
  const onKey = (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (matches.includes(norm)) attach(norm);
      else if (canCreate) attach(norm);
      else if (matches.length) attach(matches[0]);
    }
  };

  return (
    <div ref={ref} className="surface" style={{ position: "absolute", top: "100%", [align === "right" ? "right" : "left"]: 0, marginTop: 6,
      zIndex: 40, width: 244, padding: 8, boxShadow: "var(--shadow)", display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "6px 9px", borderRadius: "var(--r-sm)", background: "var(--panel-2)", border: "1px solid var(--hair)" }}>
        <Icon name="search" size="sm" style={{ color: "var(--ink-4)", flex: "none" }} />
        <input ref={inputRef} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={onKey} placeholder="Search or create a tag…"
          style={{ flex: 1, minWidth: 0, background: "transparent", border: "none", outline: "none", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5 }} />
      </div>
      {q && norm !== q.trim().toLowerCase() && (
        <div style={{ fontSize: 9.5, color: "var(--ink-4)", padding: "0 2px" }}>
          saves as <span className="tnum" style={{ color: "var(--accent-2)" }}>{norm || "…"}</span>
        </div>
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 2, maxHeight: 208, overflowY: "auto" }} className="scroll-y">
        {canCreate && (
          <button className="tag-row" onClick={() => attach(norm)}>
            <Icon name="plus" size="sm" style={{ color: "var(--accent)", flex: "none" }} />
            <span>Create</span><span className="tnum" style={{ color: "var(--accent)" }}>{norm}</span>
          </button>
        )}
        {matches.map((t) => (
          <button key={t} className="tag-row" onClick={() => attach(t)}>
            <Icon name="tag" size="sm" style={{ color: "var(--ink-4)", flex: "none" }} />
            <span className="tnum">{t}</span>
          </button>
        ))}
        {!matches.length && !canCreate && (
          <div style={{ padding: "8px 9px", fontSize: 10.5, color: "var(--ink-4)" }}>
            {available.length ? "no matches" : (attached.length ? "all tags attached" : "no tags yet — type to create one")}
          </div>
        )}
      </div>
    </div>
  );
}

// ── attached chips + an "Add" trigger that opens the picker ──────────────────
function TagBar({ allTags, tags, onChange, align = "left" }) {
  const [open, setOpen] = useStateTG(false);
  const list = tags || [];
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
      {list.map((t) => <Tag key={t} name={t} onRemove={(n) => onChange(list.filter((x) => x !== n))} />)}
      <div style={{ position: "relative" }}>
        <button className="btn ghost-hair" onClick={() => setOpen((o) => !o)}
          style={{ padding: "3px 9px", fontSize: 10.5, color: "var(--ink-3)", gap: 5 }}>
          <Icon name="plus" size="sm" style={{ width: 11, height: 11 }} />{list.length ? "Add" : "Add tag"}
        </button>
        {open && <TagPicker allTags={allTags} attached={list} onChange={onChange} onClose={() => setOpen(false)} align={align} />}
      </div>
    </div>
  );
}

// ── header filter dropdown: multi-select tags (matches ANY) ──────────────────
function TagFilter({ allTags, counts, selected, onChange }) {
  const [open, setOpen] = useStateTG(false);
  const ref = useRefTG(null);
  useEffectTG(() => {
    if (!open) return;
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const sel = selected || [];
  const toggle = (t) => onChange(sel.includes(t) ? sel.filter((x) => x !== t) : [...sel, t]);
  const n = sel.length;

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button className="btn ghost-hair" onClick={() => setOpen((o) => !o)}
        style={{ fontSize: 11.5, color: n ? "var(--accent)" : "var(--ink-2)", borderColor: n ? "color-mix(in oklch, var(--accent), transparent 58%)" : "var(--hair)" }}>
        <Icon name="tag" size="sm" style={{ color: n ? "var(--accent)" : "var(--ink-3)" }} />
        {n ? n + " tag" + (n > 1 ? "s" : "") : "Tags"}
        <Icon name="chevronD" size="sm" style={{ color: "var(--ink-4)" }} />
      </button>
      {open && (
        <div className="surface" style={{ position: "absolute", top: "100%", right: 0, marginTop: 6, zIndex: 20, padding: 6, minWidth: 214, boxShadow: "var(--shadow)" }}>
          {!(allTags || []).length && <div style={{ padding: "9px 10px", fontSize: 11, color: "var(--ink-4)" }}>No tags yet</div>}
          {n > 0 && (
            <button className="tag-row" onClick={() => onChange([])} style={{ color: "var(--ink-3)" }}>
              <Icon name="x" size="sm" style={{ flex: "none" }} />Clear filter<span className="tag-count">{n} on</span>
            </button>
          )}
          {(n > 0 && (allTags || []).length > 0) && <div style={{ height: 1, background: "var(--hair)", margin: "5px 2px" }} />}
          <div style={{ maxHeight: 280, overflowY: "auto", display: "flex", flexDirection: "column", gap: 2 }} className="scroll-y">
            {(allTags || []).map((t) => {
              const on = sel.includes(t);
              return (
                <button key={t} className="tag-row" onClick={() => toggle(t)} style={on ? { color: "var(--ink)" } : null}>
                  <span className={"tag-check" + (on ? " on" : "")}>{on && <Icon name="check" size="sm" style={{ width: 10, height: 10 }} />}</span>
                  <span className="tnum">{t}</span>
                  <span className="tag-count">{(counts && counts[t]) || 0}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

Object.assign(window, { Tag, TagPicker, TagBar, TagFilter, snakeTag, allTagsOf });
