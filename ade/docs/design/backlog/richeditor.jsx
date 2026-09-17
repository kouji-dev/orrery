/* global React, Icon */
// ── RichEditor — a contentEditable rich text editor with a formatting toolbar
// Used for ticket notes ("what should be done") and the comments composer.
const { useState: useStateRE, useRef: useRefRE, useEffect: useEffectRE, useCallback: useCbRE } = React;

// editor content typography — injected once, scoped to .rte-content
if (typeof document !== "undefined" && !document.getElementById("rte-styles")) {
  const s = document.createElement("style");
  s.id = "rte-styles";
  s.textContent = `
  .rte-content { font-family: var(--font-mono); font-size: 12.5px; line-height: 1.6; color: var(--ink-2); outline: none; word-break: break-word; }
  .rte-content:empty::before { content: attr(data-ph); color: var(--ink-4); pointer-events: none; }
  .rte-content > *:first-child { margin-top: 0; }
  .rte-content > *:last-child { margin-bottom: 0; }
  .rte-content p { margin: 0 0 8px; }
  .rte-content h2 { font-family: var(--font-disp); font-size: 16px; font-weight: 600; letter-spacing: -0.01em; color: var(--ink); margin: 14px 0 7px; }
  .rte-content h3 { font-family: var(--font-disp); font-size: 13.5px; font-weight: 600; color: var(--ink); margin: 12px 0 6px; }
  .rte-content ul, .rte-content ol { margin: 0 0 8px; padding-left: 20px; }
  .rte-content li { margin: 2px 0; }
  .rte-content a { color: var(--accent-2); text-decoration: underline; text-underline-offset: 2px; }
  .rte-content code { font-family: var(--font-mono); font-size: 0.92em; background: var(--panel-3); border: 1px solid var(--hair); border-radius: 4px; padding: 1px 5px; color: var(--accent-3); }
  .rte-content pre { background: var(--bg); border: 1px solid var(--hair); border-radius: var(--r-md); padding: 10px 12px; margin: 0 0 8px; overflow-x: auto; }
  .rte-content pre code { background: none; border: none; padding: 0; color: var(--ink-2); }
  .rte-content blockquote { margin: 0 0 8px; padding: 4px 12px; border-left: 2px solid color-mix(in oklch, var(--accent), transparent 40%); color: var(--ink-3); }
  .rte-content strong { color: var(--ink); font-weight: 700; }
  .rte-tb-btn { width: 27px; height: 27px; border-radius: var(--r-sm); border: 1px solid transparent; background: transparent; color: var(--ink-3); cursor: pointer; display: grid; place-items: center; font-family: var(--font-mono); font-size: 12px; transition: background .12s, color .12s, border-color .12s; flex: none; }
  .rte-tb-btn:hover { background: var(--panel-3); color: var(--ink); }
  .rte-tb-btn.on { background: color-mix(in oklch, var(--accent), transparent 86%); color: var(--accent); border-color: color-mix(in oklch, var(--accent), transparent 60%); }
  `;
  document.head.appendChild(s);
}

function TBDivider() { return <span style={{ width: 1, height: 16, background: "var(--hair)", margin: "0 3px", flex: "none" }} />; }

// glyph-based toolbar buttons (avoid relying on icon set for type controls)
function TBtn({ label, on, onMouseDown, children, title }) {
  return (
    <button type="button" className={"rte-tb-btn" + (on ? " on" : "")} title={title || label}
      onMouseDown={(e) => { e.preventDefault(); onMouseDown(); }}>{children}</button>
  );
}

function RichEditor({ value = "", onChange, placeholder = "Write something…", minHeight = 120, autoFocus, compact, resetSignal }) {
  const ref = useRefRE(null);
  const [, force] = useStateRE(0);
  const [linkOpen, setLinkOpen] = useStateRE(false);
  const [linkUrl, setLinkUrl] = useStateRE("");
  const savedRange = useRefRE(null);

  // seed initial HTML on mount and whenever resetSignal changes (e.g. after submit)
  useEffectRE(() => {
    if (ref.current) { ref.current.innerHTML = value || ""; }
    if (autoFocus && ref.current) ref.current.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resetSignal]);

  const sync = useCbRE(() => { onChange && onChange(ref.current ? ref.current.innerHTML : ""); force((n) => n + 1); }, [onChange]);

  const exec = (cmd, arg) => { ref.current.focus(); document.execCommand(cmd, false, arg); sync(); };
  const isOn = (cmd) => { try { return document.queryCommandState(cmd); } catch (e) { return false; } };
  const curBlock = () => { try { return (document.queryCommandValue("formatBlock") || "").toLowerCase(); } catch (e) { return ""; } };
  const setBlock = (tag) => { const cur = curBlock(); exec("formatBlock", (cur === tag || cur === tag.replace("h", "heading ")) ? "p" : tag); };

  const toggleInlineCode = () => {
    ref.current.focus();
    const sel = window.getSelection();
    if (!sel.rangeCount) return;
    const node = sel.anchorNode;
    const inCode = node && (node.nodeName === "CODE" || (node.parentElement && node.parentElement.closest("code")));
    if (inCode) {
      const codeEl = node.nodeName === "CODE" ? node : node.parentElement.closest("code");
      const text = document.createTextNode(codeEl.textContent);
      codeEl.replaceWith(text);
    } else {
      const txt = sel.toString();
      if (!txt) return;
      document.execCommand("insertHTML", false, "<code>" + txt.replace(/</g, "&lt;") + "</code>");
    }
    sync();
  };

  const openLink = () => {
    const sel = window.getSelection();
    if (sel.rangeCount) savedRange.current = sel.getRangeAt(0).cloneRange();
    setLinkUrl(""); setLinkOpen(true);
  };
  const applyLink = () => {
    if (savedRange.current) {
      const sel = window.getSelection(); sel.removeAllRanges(); sel.addRange(savedRange.current);
    }
    let url = linkUrl.trim();
    if (url) { if (!/^https?:|^mailto:/.test(url)) url = "https://" + url; exec("createLink", url); }
    setLinkOpen(false);
  };

  const onKeyDown = (e) => {
    const meta = e.metaKey || e.ctrlKey;
    if (meta && e.key.toLowerCase() === "b") { e.preventDefault(); exec("bold"); }
    else if (meta && e.key.toLowerCase() === "i") { e.preventDefault(); exec("italic"); }
    else if (meta && e.key.toLowerCase() === "k") { e.preventDefault(); openLink(); }
    else if (meta && e.shiftKey && e.key === "7") { e.preventDefault(); exec("insertOrderedList"); }
    else if (meta && e.shiftKey && e.key === "8") { e.preventDefault(); exec("insertUnorderedList"); }
  };

  const block = curBlock();
  const G = { fontFamily: "var(--font-disp)" };

  return (
    <div style={{ border: "1px solid var(--hair)", borderRadius: "var(--r-md)", background: "var(--panel-2)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 2, padding: "5px 6px", borderBottom: "1px solid var(--hair)", flexWrap: "wrap", background: "var(--panel)" }}>
        <TBtn title="Heading" on={block === "h2"} onMouseDown={() => setBlock("h2")}><span style={{ ...G, fontWeight: 700, fontSize: 13 }}>H</span></TBtn>
        <TBtn title="Subheading" on={block === "h3"} onMouseDown={() => setBlock("h3")}><span style={{ ...G, fontWeight: 700, fontSize: 11 }}>H₃</span></TBtn>
        <TBDivider />
        <TBtn title="Bold (⌘B)" on={isOn("bold")} onMouseDown={() => exec("bold")}><span style={{ fontWeight: 800 }}>B</span></TBtn>
        <TBtn title="Italic (⌘I)" on={isOn("italic")} onMouseDown={() => exec("italic")}><span style={{ fontStyle: "italic", fontFamily: "Georgia, serif" }}>I</span></TBtn>
        <TBtn title="Strikethrough" on={isOn("strikeThrough")} onMouseDown={() => exec("strikeThrough")}><span style={{ textDecoration: "line-through" }}>S</span></TBtn>
        <TBtn title="Inline code" onMouseDown={toggleInlineCode}><span style={{ fontSize: 13 }}>{"</>"}</span></TBtn>
        <TBDivider />
        <TBtn title="Bulleted list" on={isOn("insertUnorderedList")} onMouseDown={() => exec("insertUnorderedList")}><Icon name="dots" size="sm" /></TBtn>
        <TBtn title="Numbered list" on={isOn("insertOrderedList")} onMouseDown={() => exec("insertOrderedList")}><span style={{ fontSize: 11, letterSpacing: "-1px" }}>1.</span></TBtn>
        <TBtn title="Quote" on={block === "blockquote"} onMouseDown={() => setBlock("blockquote")}><span style={{ fontFamily: "Georgia, serif", fontSize: 16, lineHeight: 1 }}>"</span></TBtn>
        <TBtn title="Code block" on={block === "pre"} onMouseDown={() => setBlock("pre")}><Icon name="terminal" size="sm" /></TBtn>
        <TBDivider />
        <div style={{ position: "relative" }}>
          <TBtn title="Link (⌘K)" on={linkOpen} onMouseDown={openLink}><Icon name="link" size="sm" /></TBtn>
          {linkOpen && (
            <div onMouseDown={(e) => e.stopPropagation()} style={{ position: "absolute", top: "calc(100% + 6px)", left: 0, zIndex: 20,
              display: "flex", gap: 6, padding: 6, background: "var(--elev)", border: "1px solid var(--hair-2)", borderRadius: "var(--r-md)", boxShadow: "var(--shadow)", width: 250 }}>
              <input autoFocus value={linkUrl} onChange={(e) => setLinkUrl(e.target.value)} placeholder="https://…"
                onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); applyLink(); } if (e.key === "Escape") setLinkOpen(false); }}
                style={{ flex: 1, minWidth: 0, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "5px 8px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, outline: "none" }} />
              <button className="btn primary" style={{ padding: "4px 9px" }} onMouseDown={(e) => { e.preventDefault(); applyLink(); }}>Add</button>
            </div>
          )}
        </div>
      </div>
      <div ref={ref} className="rte-content scroll-y" contentEditable suppressContentEditableWarning
        data-ph={placeholder}
        onInput={sync} onKeyUp={() => force((n) => n + 1)} onMouseUp={() => force((n) => n + 1)} onKeyDown={onKeyDown}
        onBlur={sync}
        style={{ minHeight, maxHeight: compact ? 220 : 460, padding: compact ? "9px 11px" : "12px 14px" }} />
    </div>
  );
}

// read-only render of stored rich HTML (notes preview / posted comments)
function RichView({ html, style }) {
  return <div className="rte-content" style={{ color: "var(--ink-2)", ...style }} dangerouslySetInnerHTML={{ __html: html || "" }} />;
}

Object.assign(window, { RichEditor, RichView });
