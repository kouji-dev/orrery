/* @ds-bundle: {"format":4,"namespace":"Vortex_469485","components":[{"name":"PrismLogo","sourcePath":"components/brand/PrismLogo.jsx"},{"name":"Wordmark","sourcePath":"components/brand/Wordmark.jsx"},{"name":"BrandLockup","sourcePath":"components/brand/Wordmark.jsx"},{"name":"ChatMessage","sourcePath":"components/chat/ChatMessage.jsx"},{"name":"ThinkBlock","sourcePath":"components/chat/ChatMessage.jsx"},{"name":"ToolCard","sourcePath":"components/chat/ChatMessage.jsx"},{"name":"Composer","sourcePath":"components/chat/Composer.jsx"},{"name":"Button","sourcePath":"components/core/Button.jsx"},{"name":"IconButton","sourcePath":"components/core/Button.jsx"},{"name":"Icon","sourcePath":"components/core/Icon.jsx"},{"name":"DataTable","sourcePath":"components/data/DataTable.jsx"},{"name":"ProviderMark","sourcePath":"components/data/ModelChip.jsx"},{"name":"ProviderChip","sourcePath":"components/data/ModelChip.jsx"},{"name":"ModelChip","sourcePath":"components/data/ModelChip.jsx"},{"name":"StatusPill","sourcePath":"components/feedback/StatusPill.jsx"},{"name":"ThinkingDots","sourcePath":"components/feedback/StatusPill.jsx"},{"name":"ToolChip","sourcePath":"components/feedback/StatusPill.jsx"},{"name":"Field","sourcePath":"components/forms/Input.jsx"},{"name":"Input","sourcePath":"components/forms/Input.jsx"},{"name":"Textarea","sourcePath":"components/forms/Input.jsx"},{"name":"Select","sourcePath":"components/forms/Input.jsx"},{"name":"Checkbox","sourcePath":"components/forms/Input.jsx"},{"name":"Badge","sourcePath":"components/indicators/Badge.jsx"},{"name":"Tag","sourcePath":"components/indicators/Badge.jsx"},{"name":"Switch","sourcePath":"components/indicators/Switch.jsx"},{"name":"Avatar","sourcePath":"components/indicators/Switch.jsx"},{"name":"Tabs","sourcePath":"components/navigation/Tabs.jsx"},{"name":"FilterChip","sourcePath":"components/navigation/Tabs.jsx"},{"name":"SidebarItem","sourcePath":"components/navigation/Tabs.jsx"},{"name":"Card","sourcePath":"components/surfaces/Card.jsx"},{"name":"Panel","sourcePath":"components/surfaces/Card.jsx"},{"name":"Sparkline","sourcePath":"components/surfaces/Stat.jsx"},{"name":"Stat","sourcePath":"components/surfaces/Stat.jsx"}],"sourceHashes":{"components/brand/PrismLogo.jsx":"431c8eb5d660","components/brand/Wordmark.jsx":"d758562c5b34","components/chat/ChatMessage.jsx":"be83f82975cd","components/chat/Composer.jsx":"27c9cc0536e3","components/core/Button.jsx":"ae558d9fef69","components/core/Icon.jsx":"dc11ee699d3f","components/data/DataTable.jsx":"29ac23928f0d","components/data/ModelChip.jsx":"5ecce1f21976","components/feedback/StatusPill.jsx":"c83a19e15603","components/forms/Input.jsx":"05b5bde430c6","components/indicators/Badge.jsx":"ada06b559f47","components/indicators/Switch.jsx":"6eeb1149751a","components/navigation/Tabs.jsx":"6d8834a6f5aa","components/surfaces/Card.jsx":"07b879632925","components/surfaces/Stat.jsx":"56d0b7d3ca9d","ui_kits/app/app-kit.jsx":"84b0b8317e29","ui_kits/marketing/marketing-kit.jsx":"be7b5483325a"},"inlinedExternals":[],"unexposedExports":[]} */

(() => {

const __ds_ns = (window.Vortex_469485 = window.Vortex_469485 || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/brand/PrismLogo.jsx
try { (() => {
/**
 * PrismLogo — Vortex's brand mark. A diamond prism refracting three rays
 * (pink → violet → blue) from its top vertex, with a luminous core.
 * Animates by state to mirror the app's request lifecycle.
 */
function PrismLogo({
  state = 'idle',
  size = 64,
  title = 'Vortex',
  className = '',
  style
}) {
  const uid = React.useId().replace(/:/g, '');
  const gid = `vx-prism-${uid}`;
  const mono = state === 'mono-white' || state === 'mono-dark';
  const monoColor = state === 'mono-white' ? '#e0d7ff' : '#1a1830';
  const stroke = mono ? monoColor : `url(#${gid})`;
  const rayOpacity = mono ? 0.42 : 0.62;
  const coreFill = state === 'mono-dark' ? '#1a1830' : '#e0d7ff';

  // ray palette shifts for thinking (amber) and error (red)
  let rays = ['#f472b6', '#a78bfa', '#60a5fa'];
  if (state === 'thinking') rays = ['#fbbf24', '#f59e0b', '#fde68a'];
  if (state === 'error') rays = ['#f87171', '#ef4444', '#fca5a5'];
  if (mono) rays = [monoColor, monoColor, monoColor];
  const stroded = state === 'error' ? '#ef4444' : state === 'thinking' ? '#fbbf24' : stroke;
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-prism vx-prism-${state} ${className}`,
    style: {
      display: 'inline-block',
      width: size,
      height: size,
      ...style
    },
    role: "img",
    "aria-label": title
  }, /*#__PURE__*/React.createElement("svg", {
    viewBox: "0 0 80 80",
    width: size,
    height: size,
    style: {
      display: 'block'
    }
  }, /*#__PURE__*/React.createElement("defs", null, /*#__PURE__*/React.createElement("linearGradient", {
    id: gid,
    x1: "0",
    y1: "0",
    x2: "1",
    y2: "1"
  }, /*#__PURE__*/React.createElement("stop", {
    offset: "0%",
    stopColor: "#f472b6"
  }), /*#__PURE__*/React.createElement("stop", {
    offset: "50%",
    stopColor: "#a78bfa"
  }), /*#__PURE__*/React.createElement("stop", {
    offset: "100%",
    stopColor: "#60a5fa"
  }))), /*#__PURE__*/React.createElement("g", {
    className: "vx-prism-box"
  }, /*#__PURE__*/React.createElement("polygon", {
    points: "40,8 68,40 40,72 12,40",
    fill: "none",
    stroke: stroded,
    strokeWidth: size <= 20 ? 3 : 2.5,
    strokeLinejoin: "round"
  }), /*#__PURE__*/React.createElement("line", {
    x1: "40",
    y1: "8",
    x2: "68",
    y2: "40",
    stroke: rays[0],
    strokeWidth: "1.5",
    opacity: rayOpacity
  }), /*#__PURE__*/React.createElement("line", {
    x1: "40",
    y1: "8",
    x2: "40",
    y2: "72",
    stroke: rays[1],
    strokeWidth: "1.5",
    opacity: rayOpacity
  }), /*#__PURE__*/React.createElement("line", {
    x1: "40",
    y1: "8",
    x2: "12",
    y2: "40",
    stroke: rays[2],
    strokeWidth: "1.5",
    opacity: rayOpacity
  }), /*#__PURE__*/React.createElement("circle", {
    className: "vx-prism-core",
    cx: "40",
    cy: "40",
    r: size <= 20 ? 5 : 4,
    fill: coreFill
  }))), /*#__PURE__*/React.createElement("style", null, `
        .vx-prism .vx-prism-box { transform-origin: 40px 40px; }
        .vx-prism-idle .vx-prism-box { animation: vxPrismSway 4s ease-in-out infinite; }
        .vx-prism-loading .vx-prism-box { animation: vxPrismSpin 1.2s linear infinite; }
        .vx-prism-streaming .vx-prism-box { animation: vxPrismPend 1.8s ease-in-out infinite; }
        .vx-prism-thinking .vx-prism-box { animation: vxPrismPend 3.5s ease-in-out infinite; }
        .vx-prism-error .vx-prism-box { animation: vxPrismShake 0.5s ease-in-out infinite; }
        .vx-prism-streaming .vx-prism-core { animation: vxPrismCore 1.8s ease-in-out infinite; }
        @keyframes vxPrismSway { 0%,100% { transform: rotate(-5deg); } 50% { transform: rotate(5deg); } }
        @keyframes vxPrismSpin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
        @keyframes vxPrismPend { 0% { transform: rotate(-18deg); } 50% { transform: rotate(18deg); } 100% { transform: rotate(-18deg); } }
        @keyframes vxPrismCore { 0%,100% { r: 4; filter: drop-shadow(0 0 2px #a78bfa); } 50% { r: 7; filter: drop-shadow(0 0 8px #a78bfa); } }
        @keyframes vxPrismShake {
          0%,100% { transform: translateX(0) rotate(0); }
          15% { transform: translateX(-3px) rotate(-3deg); }
          45% { transform: translateX(3px) rotate(3deg); }
          75% { transform: translateX(-2px) rotate(-2deg); }
        }
        @media (prefers-reduced-motion: reduce) {
          .vx-prism .vx-prism-box, .vx-prism .vx-prism-core { animation: none !important; }
        }
      `));
}
Object.assign(__ds_scope, { PrismLogo });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/brand/PrismLogo.jsx", error: String((e && e.message) || e) }); }

// components/brand/Wordmark.jsx
try { (() => {
/**
 * Wordmark — "Vortex" set in the display face. `gradient` paints the
 * brand spectrum; `solid` uses a single ink for small / low-contrast use.
 */
function Wordmark({
  variant = 'gradient',
  size = 18,
  className = '',
  style
}) {
  const base = {
    fontFamily: "var(--vx-font-display)",
    fontWeight: 700,
    fontSize: size,
    letterSpacing: '-0.03em',
    lineHeight: 1
  };
  const paint = variant === 'gradient' ? {
    background: 'var(--vx-grad)',
    WebkitBackgroundClip: 'text',
    backgroundClip: 'text',
    color: 'transparent'
  } : variant === 'ink' ? {
    color: 'var(--vx-ink)'
  } : {
    color: 'var(--vx-core)'
  };
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-wordmark ${className}`,
    style: {
      ...base,
      ...paint,
      ...style
    }
  }, "Vortex");
}

/**
 * BrandLockup — Prism mark + wordmark, horizontally locked. The default
 * brand signature for nav bars, sidebars, and headers.
 */
function BrandLockup({
  size = 22,
  variant = 'gradient',
  state = 'idle',
  gap = 10,
  className = '',
  style
}) {
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-lockup ${className}`,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap,
      ...style
    }
  }, /*#__PURE__*/React.createElement(__ds_scope.PrismLogo, {
    state: state,
    size: size
  }), /*#__PURE__*/React.createElement(Wordmark, {
    variant: variant,
    size: size * 0.82
  }));
}
Object.assign(__ds_scope, { Wordmark, BrandLockup });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/brand/Wordmark.jsx", error: String((e && e.message) || e) }); }

// components/chat/ChatMessage.jsx
try { (() => {
/**
 * ChatMessage — one turn in a thread. `role` = 'user' | 'ai'. AI turns use the
 * animated Prism as the avatar; pass `state` to reflect activity. `grounded`
 * shows the green KB indicator. Children are the message body.
 */
function ChatMessage({
  role = 'ai',
  who,
  time,
  grounded,
  state = 'idle',
  children,
  className = '',
  style
}) {
  const isUser = role === 'user';
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-msg vx-msg-${role} ${className}`,
    style: {
      display: 'flex',
      gap: 10,
      maxWidth: 780,
      ...style
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 26,
      height: 26,
      flexShrink: 0,
      display: 'grid',
      placeItems: 'center'
    }
  }, isUser ? /*#__PURE__*/React.createElement("span", {
    style: {
      width: 26,
      height: 26,
      borderRadius: '50%',
      background: 'var(--vx-panel)',
      border: '1px solid var(--vx-line-2)',
      color: 'var(--vx-ink-2)',
      display: 'grid',
      placeItems: 'center',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      fontWeight: 600
    }
  }, "You") : /*#__PURE__*/React.createElement(__ds_scope.PrismLogo, {
    state: state,
    size: 22
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 6,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)',
      marginBottom: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-2)'
    }
  }, who || (isUser ? 'You' : 'Claude')), time && /*#__PURE__*/React.createElement("span", null, "\xB7 ", time), grounded && /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-good)',
      display: 'inline-flex',
      alignItems: 'center',
      gap: 4,
      marginLeft: 2
    },
    title: "Grounded in knowledge bases"
  }, /*#__PURE__*/React.createElement("svg", {
    width: "11",
    height: "11",
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "1.5"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M3.5 2.5h7a1 1 0 0 1 1 1V13l-4.5-2.5L2.5 13V3.5a1 1 0 0 1 1-1z"
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 13.5,
      lineHeight: 1.55,
      color: isUser ? 'var(--vx-ink-2)' : 'var(--vx-ink)'
    }
  }, children)));
}

/**
 * ThinkBlock — the collapsible "Thinking" container that holds ToolCards.
 * `done` collapses it and swaps the pulse for a check.
 */
function ThinkBlock({
  meta,
  done,
  children,
  defaultCollapsed,
  className = ''
}) {
  const [collapsed, setCollapsed] = React.useState(defaultCollapsed ?? false);
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-think ${className}`,
    style: {
      marginBottom: 10,
      borderRadius: 8,
      border: '1px solid var(--vx-line)',
      background: 'var(--vx-bg-2)',
      overflow: 'hidden'
    }
  }, /*#__PURE__*/React.createElement("div", {
    onClick: () => done && setCollapsed(c => !c),
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 8,
      padding: '8px 10px',
      cursor: done ? 'pointer' : 'default'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 7,
      height: 7,
      borderRadius: '50%',
      background: done ? 'var(--vx-good)' : 'var(--vx-violet)',
      boxShadow: done ? '0 0 0 2px rgba(var(--vx-good-rgb),0.18)' : '0 0 8px rgba(var(--vx-violet-rgb),0.6)',
      animation: done ? 'none' : 'vx-pulse 1.4s ease-in-out infinite'
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink)'
    }
  }, "Thinking"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10.5,
      color: 'var(--vx-ink-3)',
      marginLeft: 'auto'
    }
  }, meta)), !(done && collapsed) && /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      gap: 4,
      padding: '0 10px 10px'
    }
  }, children));
}
const ICONS = {
  memory: {
    bg: 'var(--vx-tool-memory-bg)',
    fg: 'var(--vx-tool-memory-fg)',
    paths: ['M8 13c-3 0-5-2-5-4.5S5 4 8 4s5 2 5 4.5S11 13 8 13z', 'M5 8.5c0-1 .8-1.5 1.5-1.5M11 8.5c0-1-.8-1.5-1.5-1.5']
  },
  kb: {
    bg: 'var(--vx-tool-kb-bg)',
    fg: 'var(--vx-tool-kb-fg)',
    paths: ['M3 3.5A1.5 1.5 0 0 1 4.5 2h7A1.5 1.5 0 0 1 13 3.5v9a.5.5 0 0 1-.8.4L8 10.6 3.8 12.9a.5.5 0 0 1-.8-.4v-9z']
  },
  web: {
    bg: 'var(--vx-tool-web-bg)',
    fg: 'var(--vx-tool-web-fg)',
    paths: ['M8 2a6 6 0 1 0 0 12 6 6 0 0 0 0-12z', 'M2 8h12', 'M8 2c2 2 3 4 3 6s-1 4-3 6c-2-2-3-4-3-6s1-4 3-6z']
  },
  tool: {
    bg: 'var(--vx-accent-soft)',
    fg: 'var(--vx-accent)',
    paths: ['M10 3a3 3 0 0 1 3 4L7 13a2 2 0 1 1-3-3l6-6A3 3 0 0 1 10 3Z']
  }
};

/**
 * ToolCard — a single tool invocation inside a ThinkBlock: icon, name, args,
 * and a spinner→result status.
 */
function ToolCard({
  kind = 'tool',
  name,
  args,
  result,
  done,
  className = ''
}) {
  const t = ICONS[kind] || ICONS.tool;
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-toolcard ${className}`,
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: '7px 10px',
      borderRadius: 6,
      background: done ? 'rgba(var(--vx-good-rgb),0.04)' : 'var(--vx-bg-3)',
      border: `1px solid ${done ? 'rgba(var(--vx-good-rgb),0.2)' : 'var(--vx-line-2)'}`
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 22,
      height: 22,
      borderRadius: 5,
      display: 'grid',
      placeItems: 'center',
      background: t.bg,
      color: t.fg,
      flexShrink: 0
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "12",
    height: "12",
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "1.5",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }, t.paths.map((d, i) => /*#__PURE__*/React.createElement("path", {
    key: i,
    d: d
  })))), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink)'
    }
  }, name), args && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10.5,
      color: 'var(--vx-ink-3)',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap'
    }
  }, args), /*#__PURE__*/React.createElement("span", {
    style: {
      marginLeft: 'auto',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: done ? 'var(--vx-good)' : 'var(--vx-ink-3)',
      display: 'inline-flex',
      alignItems: 'center',
      gap: 5,
      flexShrink: 0
    }
  }, done ? /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement("svg", {
    width: "10",
    height: "10",
    viewBox: "0 0 10 10",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2"
  }, /*#__PURE__*/React.createElement("polyline", {
    points: "2,5 4,7 8,3"
  })), result) : /*#__PURE__*/React.createElement("span", {
    style: {
      width: 10,
      height: 10,
      borderRadius: '50%',
      border: '1.5px solid var(--vx-line-2)',
      borderTopColor: 'var(--vx-violet)',
      animation: 'vx-spin 0.9s linear infinite',
      display: 'inline-block'
    }
  })));
}
Object.assign(__ds_scope, { ChatMessage, ThinkBlock, ToolCard });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/chat/ChatMessage.jsx", error: String((e && e.message) || e) }); }

// components/core/Button.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
const SIZES = {
  sm: {
    height: 'var(--vx-control-sm)',
    padding: '0 8px',
    fontSize: 11
  },
  md: {
    height: 'var(--vx-control-h)',
    padding: '0 12px',
    fontSize: 12
  },
  lg: {
    height: 36,
    padding: '0 16px',
    fontSize: 13
  }
};

/**
 * Button — Vortex's action control. Variants map to hierarchy:
 * `gradient` (primary CTA, brand spectrum) · `primary` (solid ink) ·
 * `default` (bordered) · `ghost` (quiet) · `danger`.
 */
function Button({
  variant = 'default',
  size = 'md',
  gradient,
  disabled,
  iconLeft,
  iconRight,
  children,
  className = '',
  style,
  ...rest
}) {
  const v = gradient ? 'gradient' : variant;
  const sz = SIZES[size] || SIZES.md;
  const base = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
    height: sz.height,
    padding: sz.padding,
    fontSize: sz.fontSize,
    fontFamily: 'var(--vx-font-sans)',
    fontWeight: 500,
    whiteSpace: 'nowrap',
    borderRadius: 'var(--vx-radius-sm)',
    border: '1px solid var(--vx-line)',
    background: 'var(--vx-panel)',
    color: 'var(--vx-ink)',
    cursor: disabled ? 'default' : 'pointer',
    opacity: disabled ? 0.45 : 1,
    transition: 'background var(--vx-dur-fast), border-color var(--vx-dur-fast), transform var(--vx-dur-fast), box-shadow var(--vx-dur-fast)'
  };
  const variants = {
    default: {},
    primary: {
      background: 'var(--vx-ink)',
      color: 'var(--vx-bg)',
      borderColor: 'var(--vx-ink)'
    },
    accent: {
      background: 'var(--vx-accent)',
      color: 'var(--vx-accent-ink)',
      borderColor: 'var(--vx-accent)'
    },
    ghost: {
      background: 'transparent',
      borderColor: 'transparent',
      color: 'var(--vx-ink-2)'
    },
    danger: {
      background: 'transparent',
      borderColor: 'rgba(var(--vx-err-rgb),0.4)',
      color: 'var(--vx-err)'
    },
    gradient: {
      background: 'var(--vx-grad)',
      color: '#0b0b16',
      borderColor: 'transparent',
      fontWeight: 600,
      boxShadow: '0 0 20px -6px rgba(var(--vx-violet-rgb),0.7)'
    }
  };
  return /*#__PURE__*/React.createElement("button", _extends({
    className: `vx-btn vx-btn-${v} ${className}`,
    disabled: disabled,
    style: {
      ...base,
      ...variants[v],
      ...style
    }
  }, rest), iconLeft, children, iconRight, /*#__PURE__*/React.createElement("style", null, `
        .vx-btn-default:not(:disabled):hover { background: var(--vx-panel-2); border-color: var(--vx-line-2); }
        .vx-btn-primary:not(:disabled):hover { background: var(--vx-ink-2); border-color: var(--vx-ink-2); }
        .vx-btn-accent:not(:disabled):hover { filter: brightness(1.06); }
        .vx-btn-ghost:not(:disabled):hover { background: var(--vx-panel-2); color: var(--vx-ink); }
        .vx-btn-danger:not(:disabled):hover { background: rgba(var(--vx-err-rgb),0.1); }
        .vx-btn-gradient:not(:disabled):hover { transform: translateY(-1px); box-shadow: 0 10px 30px -8px rgba(var(--vx-violet-rgb),0.55); filter: brightness(1.03); }
      `));
}

/**
 * IconButton — square, icon-only control. Same variants, quiet by default.
 */
function IconButton({
  variant = 'ghost',
  size = 'md',
  disabled,
  children,
  title,
  className = '',
  style,
  ...rest
}) {
  const dim = size === 'sm' ? 'var(--vx-control-sm)' : size === 'lg' ? 36 : 'var(--vx-control-h)';
  const styles = {
    ghost: {
      background: 'transparent',
      borderColor: 'transparent',
      color: 'var(--vx-ink-3)'
    },
    default: {
      background: 'var(--vx-panel)',
      borderColor: 'var(--vx-line)',
      color: 'var(--vx-ink-2)'
    },
    gradient: {
      background: 'var(--vx-grad)',
      borderColor: 'transparent',
      color: '#0b0b16',
      boxShadow: '0 0 16px -6px rgba(var(--vx-violet-rgb),0.7)'
    }
  };
  return /*#__PURE__*/React.createElement("button", _extends({
    className: `vx-iconbtn vx-iconbtn-${variant} ${className}`,
    title: title,
    "aria-label": title,
    disabled: disabled,
    style: {
      display: 'grid',
      placeItems: 'center',
      width: dim,
      height: dim,
      flexShrink: 0,
      borderRadius: 'var(--vx-radius-sm)',
      border: '1px solid transparent',
      cursor: disabled ? 'default' : 'pointer',
      opacity: disabled ? 0.45 : 1,
      transition: 'background var(--vx-dur-fast), color var(--vx-dur-fast), border-color var(--vx-dur-fast)',
      ...styles[variant],
      ...style
    }
  }, rest), children, /*#__PURE__*/React.createElement("style", null, `
        .vx-iconbtn-ghost:not(:disabled):hover { background: var(--vx-panel-2); color: var(--vx-ink); border-color: var(--vx-line); }
        .vx-iconbtn-default:not(:disabled):hover { background: var(--vx-panel-2); color: var(--vx-ink); }
      `));
}
Object.assign(__ds_scope, { Button, IconButton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Button.jsx", error: String((e && e.message) || e) }); }

// components/core/Icon.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/**
 * Icon — Vortex's line-icon set. 16px grid, 1.25 stroke, round caps/joins.
 * Matches the glyph vocabulary used across the product (nav, actions, tools).
 */
function Icon({
  name,
  size = 14,
  className = '',
  style
}) {
  const common = {
    width: size,
    height: size,
    viewBox: '0 0 16 16',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.25,
    strokeLinecap: 'round',
    strokeLinejoin: 'round',
    className: `vx-icon ${className}`,
    style
  };
  switch (name) {
    // nav / product
    case 'chat':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M2 4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H6l-3 3v-3a2 2 0 0 1-1-2V4Z"
      }));
    case 'library':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M3 3v10M6 3v10M9 3v10"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M12 3l2 10"
      }));
    case 'brain':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 2a4 4 0 0 0-4 4v1a3 3 0 0 0 0 6v1a2 2 0 0 0 2 2h4a2 2 0 0 0 2-2v-1a3 3 0 0 0 0-6V6a4 4 0 0 0-4-4Z"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M8 6v8"
      }));
    case 'cpu':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("rect", {
        x: "3",
        y: "3",
        width: "10",
        height: "10",
        rx: "1"
      }), /*#__PURE__*/React.createElement("rect", {
        x: "6",
        y: "6",
        width: "4",
        height: "4"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M1 6h2M1 10h2M13 6h2M13 10h2M6 1v2M10 1v2M6 13v2M10 13v2"
      }));
    case 'key':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "5",
        cy: "11",
        r: "2.5"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M7 9l6-6M11 5l1 1M10 6l1 1"
      }));
    case 'gov':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 2l5 2v5c0 3-2.5 4.5-5 5.5-2.5-1-5-2.5-5-5.5V4l5-2Z"
      }));
    case 'home':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M2 7l6-5 6 5v6a1 1 0 0 1-1 1h-3v-4H6v4H3a1 1 0 0 1-1-1V7Z"
      }));
    case 'team':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "6",
        cy: "6",
        r: "2.5"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M2 14c0-2 2-3.5 4-3.5s4 1.5 4 3.5"
      }), /*#__PURE__*/React.createElement("circle", {
        cx: "11",
        cy: "5",
        r: "2"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M9.5 10c3 0 4.5 1.5 4.5 3.5"
      }));
    case 'database':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("ellipse", {
        cx: "8",
        cy: "3.5",
        rx: "5",
        ry: "1.5"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M3 3.5v9c0 .8 2.2 1.5 5 1.5s5-.7 5-1.5v-9M3 8c0 .8 2.2 1.5 5 1.5s5-.7 5-1.5"
      }));
    case 'book':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("rect", {
        x: "3",
        y: "2",
        width: "10",
        height: "12",
        rx: "1"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M6 5h4M6 8h4M6 11h3"
      }));
    // actions
    case 'search':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "7",
        cy: "7",
        r: "4"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M10 10l3 3"
      }));
    case 'plus':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 3v10M3 8h10"
      }));
    case 'send':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 13V3M4 7l4-4 4 4"
      }));
    case 'sun':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "8",
        r: "3"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.5 1.5M11.5 11.5L13 13M3 13l1.5-1.5M11.5 4.5L13 3"
      }));
    case 'moon':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M13 9A6 6 0 0 1 7 3a6 6 0 1 0 6 6Z"
      }));
    case 'x':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M4 4l8 8M12 4l-8 8"
      }));
    case 'check':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M3 8l3 3 7-7"
      }));
    case 'chevron-down':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M4 6l4 4 4-4"
      }));
    case 'chevron-right':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M6 4l4 4-4 4"
      }));
    case 'chevron-updn':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M5 6l3-3 3 3M5 10l3 3 3-3"
      }));
    case 'filter':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M2 3h12l-4 5v5l-4-2V8L2 3Z"
      }));
    case 'bell':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M4 12V7a4 4 0 0 1 8 0v5M2 12h12M7 14h2"
      }));
    case 'more':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "4",
        cy: "8",
        r: "1",
        fill: "currentColor"
      }), /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "8",
        r: "1",
        fill: "currentColor"
      }), /*#__PURE__*/React.createElement("circle", {
        cx: "12",
        cy: "8",
        r: "1",
        fill: "currentColor"
      }));
    case 'more-v':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "4",
        r: "1",
        fill: "currentColor"
      }), /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "8",
        r: "1",
        fill: "currentColor"
      }), /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "12",
        r: "1",
        fill: "currentColor"
      }));
    case 'arrow-up':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 13V3M4 7l4-4 4 4"
      }));
    case 'arrow-right':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M3 8h10M9 4l4 4-4 4"
      }));
    case 'link':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M6 10l4-4M5 11a2 2 0 0 1 0-3l2-2a2 2 0 0 1 3 0M11 5a2 2 0 0 1 0 3l-2 2a2 2 0 0 1-3 0"
      }));
    case 'pin':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M9 1l6 6-2 2-1-1-3 3v3L7 12l-5 3 3-5-2-2 3-3-1-1 2-2 2 1Z"
      }));
    case 'paperclip':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M13 6l-6 6a3 3 0 0 1-4-4l6-6a2 2 0 0 1 3 3l-6 6a1 1 0 0 1-1-1l5-5"
      }));
    case 'sparkle':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 2l1 4 4 1-4 1-1 4-1-4-4-1 4-1 1-4ZM13 10l.5 2 2 .5-2 .5L13 15l-.5-2-2-.5 2-.5.5-2Z"
      }));
    case 'globe':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "8",
        r: "6"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12"
      }));
    case 'wrench':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M10 3a3 3 0 0 1 3 4L7 13a2 2 0 1 1-3-3l6-6A3 3 0 0 1 10 3Z"
      }));
    case 'upload':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 11V3M4 7l4-4 4 4M2 13h12"
      }));
    case 'copy':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("rect", {
        x: "5",
        y: "5",
        width: "9",
        height: "9",
        rx: "1"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M11 5V3a1 1 0 0 0-1-1H3a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1h2"
      }));
    case 'stop':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("rect", {
        x: "4",
        y: "4",
        width: "8",
        height: "8",
        rx: "1",
        fill: "currentColor",
        stroke: "none"
      }));
    case 'eye':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5Z"
      }), /*#__PURE__*/React.createElement("circle", {
        cx: "8",
        cy: "8",
        r: "2"
      }));
    case 'lock':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("rect", {
        x: "3",
        y: "7",
        width: "10",
        height: "7",
        rx: "1"
      }), /*#__PURE__*/React.createElement("path", {
        d: "M5 7V5a3 3 0 0 1 6 0v2"
      }));
    case 'rocket':
      return /*#__PURE__*/React.createElement("svg", common, /*#__PURE__*/React.createElement("path", {
        d: "M8 2c3 0 5 2 5 5l-2 2v4l-3-2-3 2V9L3 7c0-3 2-5 5-5ZM6 8a2 2 0 0 1 4 0"
      }));
    case 'loader':
      return /*#__PURE__*/React.createElement("svg", _extends({}, common, {
        style: {
          ...style,
          animation: 'vx-spin 0.7s linear infinite'
        }
      }), /*#__PURE__*/React.createElement("path", {
        d: "M8 1v3M8 12v3M1 8h3M12 8h3M3 3l2 2M11 11l2 2M3 13l2-2M11 5l2-2"
      }));
    default:
      return null;
  }
}
Object.assign(__ds_scope, { Icon });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Icon.jsx", error: String((e && e.message) || e) }); }

// components/data/DataTable.jsx
try { (() => {
/**
 * DataTable — a dense, monospace-friendly table for catalogs, keys, audit
 * logs. `columns` describe headers + alignment; `rows` are arrays of cells.
 * Keep it cosmetic — no sorting logic baked in.
 */
function DataTable({
  columns = [],
  rows = [],
  onRowClick,
  className = '',
  style
}) {
  return /*#__PURE__*/React.createElement("table", {
    className: `vx-table ${className}`,
    style: {
      width: '100%',
      borderCollapse: 'separate',
      borderSpacing: 0,
      fontSize: 12,
      ...style
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", null, columns.map((c, i) => /*#__PURE__*/React.createElement("th", {
    key: i,
    style: {
      textAlign: c.align || 'left',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      textTransform: 'uppercase',
      letterSpacing: '0.06em',
      color: 'var(--vx-ink-3)',
      fontWeight: 500,
      padding: '8px 12px',
      background: 'var(--vx-bg-2)',
      borderBottom: '1px solid var(--vx-line)',
      whiteSpace: 'nowrap',
      width: c.width
    }
  }, c.label)))), /*#__PURE__*/React.createElement("tbody", null, rows.map((row, ri) => /*#__PURE__*/React.createElement("tr", {
    key: ri,
    onClick: onRowClick ? () => onRowClick(ri) : undefined,
    className: "vx-table-row",
    style: {
      cursor: onRowClick ? 'pointer' : 'default'
    }
  }, columns.map((c, ci) => /*#__PURE__*/React.createElement("td", {
    key: ci,
    style: {
      padding: '9px 12px',
      borderBottom: '1px solid var(--vx-line-2)',
      textAlign: c.align || 'left',
      verticalAlign: 'middle',
      whiteSpace: 'nowrap',
      fontFamily: c.mono ? 'var(--vx-font-mono)' : 'var(--vx-font-sans)',
      color: ci === 0 ? 'var(--vx-ink)' : 'var(--vx-ink-2)',
      fontVariantNumeric: c.align === 'right' ? 'tabular-nums' : 'normal'
    }
  }, row[ci]))))), /*#__PURE__*/React.createElement("style", null, `.vx-table-row:hover { background: var(--vx-hl); }`));
}
Object.assign(__ds_scope, { DataTable });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/DataTable.jsx", error: String((e && e.message) || e) }); }

// components/data/ModelChip.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
const PROVIDERS = {
  anthropic: {
    letter: 'A',
    label: 'Anthropic',
    color: 'var(--vx-prov-anthropic)'
  },
  openai: {
    letter: 'O',
    label: 'OpenAI',
    color: 'var(--vx-prov-openai)'
  },
  google: {
    letter: 'G',
    label: 'Google',
    color: 'var(--vx-prov-google)'
  },
  mistral: {
    letter: 'M',
    label: 'Mistral',
    color: 'var(--vx-prov-mistral)'
  }
};

/** ProviderMark — the small square glyph identifying a model provider. */
function ProviderMark({
  provider,
  size = 14
}) {
  const p = PROVIDERS[provider] || {
    letter: '?',
    color: 'var(--vx-ink-4)'
  };
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-prov-mark vx-prov-${provider}`,
    style: {
      width: size,
      height: size,
      borderRadius: 3,
      display: 'grid',
      placeItems: 'center',
      fontWeight: 700,
      fontSize: Math.round(size * 0.6),
      color: '#fff',
      fontFamily: 'var(--vx-font-mono)',
      flexShrink: 0,
      background: p.color
    }
  }, p.letter);
}

/** ProviderChip — provider mark + label, in mono. */
function ProviderChip({
  provider,
  label
}) {
  const p = PROVIDERS[provider] || {};
  return /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-2)'
    }
  }, /*#__PURE__*/React.createElement(ProviderMark, {
    provider: provider
  }), label || p.label || provider);
}

/**
 * ModelChip — the compact model selector token used in the composer and
 * catalog: provider mark, model name, optional effort/reasoning + cost.
 */
function ModelChip({
  provider = 'anthropic',
  name,
  effort,
  cost,
  active,
  className = '',
  style,
  ...rest
}) {
  return /*#__PURE__*/React.createElement("span", _extends({
    className: `vx-model-chip ${className}`,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      height: 26,
      padding: '0 8px',
      borderRadius: 'var(--vx-radius-sm)',
      border: `1px solid ${active ? 'rgba(var(--vx-violet-rgb),0.4)' : 'var(--vx-line-2)'}`,
      background: active ? 'rgba(var(--vx-violet-rgb),0.1)' : 'var(--vx-bg-3)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11.5,
      color: 'var(--vx-ink-2)',
      whiteSpace: 'nowrap',
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement(ProviderMark, {
    provider: provider,
    size: 13
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontWeight: 500,
      color: 'var(--vx-ink)'
    }
  }, name), effort && /*#__PURE__*/React.createElement("span", {
    style: {
      padding: '1px 5px',
      borderRadius: 3,
      background: 'rgba(var(--vx-violet-rgb),0.12)',
      color: 'var(--vx-violet)',
      fontSize: 10
    }
  }, effort), cost && /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-3)'
    }
  }, "\xB7 ", cost));
}
Object.assign(__ds_scope, { ProviderMark, ProviderChip, ModelChip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/data/ModelChip.jsx", error: String((e && e.message) || e) }); }

// components/chat/Composer.jsx
try { (() => {
/**
 * Composer — the Vortex chat input dock: textarea, attach + capability
 * toggles (Reflection / Research), attached-KB tag, model selector, and the
 * gradient send button. Cosmetic shell — wire your own state.
 */
function Composer({
  value,
  onChange,
  onSend,
  placeholder = 'Ask anything, attach a KB with @, switch models with ⌘K…',
  capabilities = {
    reflection: true,
    research: false
  },
  onToggle,
  kbLabel,
  model = {
    provider: 'anthropic',
    name: 'Sonnet 4.6',
    effort: 'high'
  },
  className = '',
  style
}) {
  const [focus, setFocus] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-composer ${className}`,
    style: {
      borderRadius: 'var(--vx-radius-lg)',
      background: 'var(--vx-panel)',
      padding: '10px 10px 8px',
      border: `1px solid ${focus ? 'rgba(var(--vx-violet-rgb),0.35)' : 'var(--vx-line)'}`,
      boxShadow: focus ? '0 0 0 3px rgba(var(--vx-violet-rgb),0.08)' : 'var(--vx-shadow-sm)',
      transition: 'border-color var(--vx-dur-fast), box-shadow var(--vx-dur-fast)',
      ...style
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '4px 6px 8px'
    }
  }, /*#__PURE__*/React.createElement("textarea", {
    value: value,
    onChange: e => onChange?.(e.target.value),
    onFocus: () => setFocus(true),
    onBlur: () => setFocus(false),
    onKeyDown: e => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        onSend?.();
      }
    },
    placeholder: placeholder,
    rows: 1,
    style: {
      width: '100%',
      background: 'transparent',
      border: 0,
      outline: 0,
      resize: 'none',
      color: 'var(--vx-ink)',
      font: 'inherit',
      fontSize: 13.5,
      lineHeight: 1.45,
      minHeight: 22
    }
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 6
    }
  }, /*#__PURE__*/React.createElement(IconBtn, {
    title: "Attach file"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M13 6l-6 6a3 3 0 0 1-4-4l6-6a2 2 0 0 1 3 3l-6 6a1 1 0 0 1-1-1l5-5"
  })), /*#__PURE__*/React.createElement(Cap, {
    on: capabilities.reflection,
    tone: "accent",
    label: "Reflection",
    onClick: () => onToggle?.('reflection')
  }), /*#__PURE__*/React.createElement(Cap, {
    on: capabilities.research,
    tone: "blue",
    label: "Research",
    onClick: () => onToggle?.('research')
  }), kbLabel && /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 5,
      height: 24,
      padding: '0 8px',
      borderRadius: 'var(--vx-radius-sm)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      border: '1px solid var(--vx-tool-kb-bd)',
      background: 'var(--vx-tool-kb-bg)',
      color: 'var(--vx-tool-kb-fg)'
    }
  }, kbLabel, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: '50%',
      background: 'var(--vx-good)'
    }
  })), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1
    }
  }), /*#__PURE__*/React.createElement(__ds_scope.ModelChip, {
    provider: model.provider,
    name: model.name,
    effort: model.effort,
    active: true
  }), /*#__PURE__*/React.createElement("button", {
    onClick: onSend,
    "aria-label": "Send",
    style: {
      width: 28,
      height: 28,
      borderRadius: 7,
      background: 'var(--vx-accent)',
      color: 'var(--vx-accent-ink)',
      display: 'grid',
      placeItems: 'center',
      flexShrink: 0
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "14",
    height: "14",
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "1.75",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }, /*#__PURE__*/React.createElement("path", {
    d: "M8 13V3M4 7l4-4 4 4"
  })))));
}
function IconBtn({
  title,
  children
}) {
  return /*#__PURE__*/React.createElement("button", {
    title: title,
    style: {
      width: 26,
      height: 26,
      display: 'grid',
      placeItems: 'center',
      borderRadius: 6,
      background: 'transparent',
      border: '1px solid transparent',
      color: 'var(--vx-ink-3)',
      flexShrink: 0
    }
  }, /*#__PURE__*/React.createElement("svg", {
    width: "14",
    height: "14",
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "1.5",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }, children));
}
function Cap({
  on,
  tone,
  label,
  onClick
}) {
  const tones = {
    accent: {
      bd: 'var(--vx-accent)',
      bg: 'var(--vx-accent-soft)',
      fg: 'var(--vx-accent)'
    },
    blue: {
      bd: 'var(--vx-tool-memory-bd)',
      bg: 'var(--vx-tool-memory-bg)',
      fg: 'var(--vx-tool-memory-fg)'
    }
  };
  const t = tones[tone] || tones.accent;
  return /*#__PURE__*/React.createElement("button", {
    onClick: onClick,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 5,
      height: 24,
      padding: '0 8px',
      borderRadius: 'var(--vx-radius-sm)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      border: `1px solid ${on ? t.bd : 'var(--vx-line-2)'}`,
      background: on ? t.bg : 'var(--vx-bg-3)',
      color: on ? t.fg : 'var(--vx-ink-3)',
      flexShrink: 0
    }
  }, label, on && /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: '50%',
      background: 'var(--vx-good)',
      marginLeft: 2
    }
  }));
}
Object.assign(__ds_scope, { Composer });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/chat/Composer.jsx", error: String((e && e.message) || e) }); }

// components/forms/Input.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
const fieldBase = {
  width: '100%',
  padding: '7px 10px',
  background: 'var(--vx-panel)',
  border: '1px solid var(--vx-line-2)',
  borderRadius: 'var(--vx-radius-sm)',
  fontSize: 13,
  color: 'var(--vx-ink)',
  outline: 'none',
  fontFamily: 'var(--vx-font-sans)',
  transition: 'border-color var(--vx-dur-fast), box-shadow var(--vx-dur-fast)'
};
function useFocusRing() {
  const [f, setF] = React.useState(false);
  const ring = f ? {
    borderColor: 'var(--vx-accent)',
    boxShadow: 'var(--vx-glow-ring)'
  } : null;
  return [ring, {
    onFocus: () => setF(true),
    onBlur: () => setF(false)
  }];
}

/** Field — label + hint wrapper for any form control. */
function Field({
  label,
  hint,
  children,
  htmlFor,
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    className: "vx-field",
    style: {
      marginBottom: 16,
      maxWidth: 'var(--vx-max-prose)',
      ...style
    }
  }, label && /*#__PURE__*/React.createElement("label", {
    htmlFor: htmlFor,
    style: {
      display: 'block',
      fontSize: 12,
      fontWeight: 500,
      marginBottom: 4,
      color: 'var(--vx-ink)'
    }
  }, label), hint && /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      color: 'var(--vx-ink-3)',
      marginBottom: 6
    }
  }, hint), children);
}

/** Input — single-line text field. */
function Input({
  mono,
  className = '',
  style,
  ...rest
}) {
  const [ring, handlers] = useFocusRing();
  return /*#__PURE__*/React.createElement("input", _extends({
    className: `vx-input ${className}`,
    style: {
      ...fieldBase,
      fontFamily: mono ? 'var(--vx-font-mono)' : fieldBase.fontFamily,
      ...ring,
      ...style
    }
  }, handlers, rest));
}

/** Textarea — multi-line. Mono by default (prompt / config editing). */
function Textarea({
  mono = true,
  rows = 4,
  className = '',
  style,
  ...rest
}) {
  const [ring, handlers] = useFocusRing();
  return /*#__PURE__*/React.createElement("textarea", _extends({
    rows: rows,
    className: `vx-textarea ${className}`,
    style: {
      ...fieldBase,
      minHeight: 80,
      resize: 'vertical',
      lineHeight: 1.6,
      fontFamily: mono ? 'var(--vx-font-mono)' : 'var(--vx-font-sans)',
      fontSize: mono ? 12 : 13,
      ...ring,
      ...style
    }
  }, handlers, rest));
}

/** Select — native dropdown with the Vortex chevron treatment. */
function Select({
  children,
  className = '',
  style,
  ...rest
}) {
  const [ring, handlers] = useFocusRing();
  return /*#__PURE__*/React.createElement("select", _extends({
    className: `vx-select ${className}`,
    style: {
      ...fieldBase,
      appearance: 'none',
      paddingRight: 30,
      cursor: 'pointer',
      backgroundImage: 'linear-gradient(45deg, transparent 50%, var(--vx-ink-3) 50%), linear-gradient(135deg, var(--vx-ink-3) 50%, transparent 50%)',
      backgroundPosition: 'calc(100% - 15px) 50%, calc(100% - 10px) 50%',
      backgroundSize: '5px 5px',
      backgroundRepeat: 'no-repeat',
      ...ring,
      ...style
    }
  }, handlers, rest), children);
}

/** Checkbox — accent-filled tick box with an optional label. */
function Checkbox({
  checked = false,
  onChange,
  label,
  disabled,
  style
}) {
  return /*#__PURE__*/React.createElement("label", {
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 8,
      cursor: disabled ? 'default' : 'pointer',
      opacity: disabled ? 0.5 : 1,
      fontSize: 13,
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    onClick: () => !disabled && onChange?.(!checked),
    style: {
      width: 15,
      height: 15,
      borderRadius: 4,
      display: 'grid',
      placeItems: 'center',
      flexShrink: 0,
      border: `1px solid ${checked ? 'var(--vx-accent)' : 'var(--vx-line-3)'}`,
      background: checked ? 'var(--vx-accent)' : 'var(--vx-panel)',
      color: '#0b0b16',
      transition: 'background var(--vx-dur-fast), border-color var(--vx-dur-fast)'
    }
  }, checked && /*#__PURE__*/React.createElement("svg", {
    width: "10",
    height: "10",
    viewBox: "0 0 10 10",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: "2",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }, /*#__PURE__*/React.createElement("polyline", {
    points: "2,5 4,7 8,3"
  }))), label && /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-2)'
    }
  }, label));
}
Object.assign(__ds_scope, { Field, Input, Textarea, Select, Checkbox });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Input.jsx", error: String((e && e.message) || e) }); }

// components/indicators/Badge.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
const TONES = {
  neutral: {
    color: 'var(--vx-ink-2)',
    bg: 'var(--vx-bg-3)',
    bd: 'var(--vx-line-2)'
  },
  accent: {
    color: 'var(--vx-violet)',
    bg: 'rgba(var(--vx-violet-rgb),0.12)',
    bd: 'rgba(var(--vx-violet-rgb),0.35)'
  },
  pink: {
    color: 'var(--vx-pink)',
    bg: 'rgba(var(--vx-pink-rgb),0.12)',
    bd: 'rgba(var(--vx-pink-rgb),0.35)'
  },
  blue: {
    color: 'var(--vx-blue)',
    bg: 'rgba(var(--vx-blue-rgb),0.12)',
    bd: 'rgba(var(--vx-blue-rgb),0.35)'
  },
  ok: {
    color: 'var(--vx-good)',
    bg: 'rgba(var(--vx-good-rgb),0.12)',
    bd: 'rgba(var(--vx-good-rgb),0.3)'
  },
  warn: {
    color: 'var(--vx-warn)',
    bg: 'rgba(var(--vx-warn-rgb),0.12)',
    bd: 'rgba(var(--vx-warn-rgb),0.3)'
  },
  err: {
    color: 'var(--vx-err)',
    bg: 'rgba(var(--vx-err-rgb),0.12)',
    bd: 'rgba(var(--vx-err-rgb),0.3)'
  }
};

/**
 * Badge — a compact mono pill for statuses and counts. `dot` prepends a
 * status dot (recommended for ok/warn/err).
 */
function Badge({
  tone = 'neutral',
  dot = false,
  children,
  className = '',
  style
}) {
  const t = TONES[tone] || TONES.neutral;
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-badge ${className}`,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 4,
      height: 18,
      padding: '0 6px',
      borderRadius: 'var(--vx-radius-xs)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      fontWeight: 500,
      letterSpacing: '0.02em',
      textTransform: 'uppercase',
      color: t.color,
      background: t.bg,
      border: `1px solid ${t.bd}`,
      ...style
    }
  }, dot && /*#__PURE__*/React.createElement("span", {
    style: {
      width: 6,
      height: 6,
      borderRadius: '50%',
      background: t.color
    }
  }), children);
}

/**
 * Tag — a rounded capsule for filters, KB attachments, and metadata.
 * Slightly larger and softer than Badge; `dot` shows an accent indicator.
 */
function Tag({
  tone = 'neutral',
  dot = false,
  children,
  className = '',
  style,
  ...rest
}) {
  const t = TONES[tone] || TONES.neutral;
  return /*#__PURE__*/React.createElement("span", _extends({
    className: `vx-tag ${className}`,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      height: 22,
      padding: '0 9px',
      borderRadius: 'var(--vx-radius-pill)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: t.color,
      background: t.bg,
      border: `1px solid ${t.bd}`,
      whiteSpace: 'nowrap',
      ...style
    }
  }, rest), dot && /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: '50%',
      background: t.color,
      boxShadow: `0 0 0 2px ${t.bg}`
    }
  }), children);
}
Object.assign(__ds_scope, { Badge, Tag });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/indicators/Badge.jsx", error: String((e && e.message) || e) }); }

// components/feedback/StatusPill.jsx
try { (() => {
const MAP = {
  ready: 'ok',
  active: 'ok',
  healthy: 'ok',
  operational: 'ok',
  entitled: 'ok',
  indexing: 'accent',
  running: 'accent',
  preview: 'accent',
  stale: 'warn',
  queued: 'warn',
  degraded: 'warn',
  failed: 'err',
  revoked: 'err',
  blocked: 'err',
  error: 'err'
};

/**
 * StatusPill — maps a lifecycle status string to the right Badge tone + dot.
 * The single source of truth for status colours across the product.
 */
function StatusPill({
  status,
  children
}) {
  const tone = MAP[String(status).toLowerCase()] || 'neutral';
  const dot = tone === 'ok' || tone === 'warn' || tone === 'err';
  return /*#__PURE__*/React.createElement(__ds_scope.Badge, {
    tone: tone,
    dot: dot
  }, children || status);
}

/**
 * ThinkingDots — the 3-dot bounce shown between a tool resolving and the
 * first streamed token.
 */
function ThinkingDots({
  color = 'var(--vx-violet)',
  size = 6
}) {
  return /*#__PURE__*/React.createElement("span", {
    className: "vx-thinking",
    style: {
      display: 'inline-flex',
      gap: 4,
      padding: '6px 2px',
      alignItems: 'center'
    }
  }, [0, 1, 2].map(i => /*#__PURE__*/React.createElement("span", {
    key: i,
    style: {
      width: size,
      height: size,
      borderRadius: '50%',
      background: color,
      animation: `vx-bounce 1.2s ease-in-out ${i * 0.18}s infinite`
    }
  })), /*#__PURE__*/React.createElement("style", null, `@keyframes vx-bounce { 0%,80%,100% { transform: translateY(0); opacity: 0.5; } 40% { transform: translateY(-5px); opacity: 1; } }`));
}
const CHIP_TONE = {
  memory: {
    bd: 'var(--vx-tool-memory-bd)',
    bg: 'var(--vx-tool-memory-bg)',
    fg: 'var(--vx-tool-memory-fg)'
  },
  kb: {
    bd: 'var(--vx-tool-kb-bd)',
    bg: 'var(--vx-tool-kb-bg)',
    fg: 'var(--vx-tool-kb-fg)'
  },
  web: {
    bd: 'var(--vx-tool-web-bd)',
    bg: 'var(--vx-tool-web-bg)',
    fg: 'var(--vx-tool-web-fg)'
  },
  tool: {
    bd: 'var(--vx-line-2)',
    bg: 'var(--vx-bg-3)',
    fg: 'var(--vx-ink-2)'
  }
};

/**
 * ToolChip — the live in-thread status chip for a memory lookup, KB search,
 * web search, or tool call. `state` drives the spinner → check transition.
 */
function ToolChip({
  kind = 'tool',
  label,
  state = 'running',
  className = ''
}) {
  const t = CHIP_TONE[kind] || CHIP_TONE.tool;
  const done = state === 'done';
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-toolchip ${className}`,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      height: 22,
      padding: '0 8px',
      borderRadius: 'var(--vx-radius-pill)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10.5,
      border: `1px solid ${t.bd}`,
      background: t.bg,
      color: t.fg,
      whiteSpace: 'nowrap'
    }
  }, done ? /*#__PURE__*/React.createElement("svg", {
    width: "11",
    height: "11",
    viewBox: "0 0 10 10",
    fill: "none",
    stroke: "var(--vx-good)",
    strokeWidth: "2",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }, /*#__PURE__*/React.createElement("polyline", {
    points: "2,5 4,7 8,3"
  })) : /*#__PURE__*/React.createElement("span", {
    style: {
      width: 10,
      height: 10,
      borderRadius: '50%',
      border: `1.5px solid ${t.bd}`,
      borderTopColor: t.fg,
      animation: 'vx-spin 0.9s linear infinite',
      display: 'inline-block'
    }
  }), label);
}
Object.assign(__ds_scope, { StatusPill, ThinkingDots, ToolChip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/StatusPill.jsx", error: String((e && e.message) || e) }); }

// components/indicators/Switch.jsx
try { (() => {
/**
 * Switch — a compact toggle. On uses the brand accent.
 */
function Switch({
  checked = false,
  onChange,
  disabled,
  size = 'md',
  className = '',
  style
}) {
  const w = size === 'sm' ? 22 : 26;
  const h = size === 'sm' ? 12 : 14;
  const knob = h - 2;
  return /*#__PURE__*/React.createElement("span", {
    role: "switch",
    "aria-checked": checked,
    className: `vx-switch ${className}`,
    onClick: () => !disabled && onChange?.(!checked),
    style: {
      display: 'inline-block',
      width: w,
      height: h,
      borderRadius: h,
      background: checked ? 'var(--vx-accent)' : 'var(--vx-line-2)',
      position: 'relative',
      cursor: disabled ? 'default' : 'pointer',
      opacity: disabled ? 0.45 : 1,
      flexShrink: 0,
      transition: 'background var(--vx-dur-fast)',
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      position: 'absolute',
      top: 1,
      left: checked ? w - knob - 1 : 1,
      width: knob,
      height: knob,
      borderRadius: '50%',
      background: checked ? '#0b0b16' : 'var(--vx-panel)',
      transition: 'left var(--vx-dur-fast)',
      boxShadow: '0 1px 2px rgba(0,0,0,0.4)'
    }
  }));
}

// Enterprise avatar palette — cool, trustworthy hues (no brand pink).
const HUES = ['#7c8cf8', '#5b8def', '#4aa3c7', '#6b6f8f', '#5f9ea0'];

/**
 * Avatar — initials in a spectrum gradient circle, or a photo when `src` is set.
 */
function Avatar({
  name = '',
  src,
  size = 26,
  className = '',
  style
}) {
  const initials = name.split(/\s+/).filter(Boolean).slice(0, 2).map(w => w[0]).join('').toUpperCase() || '?';
  const seed = [...name].reduce((a, c) => a + c.charCodeAt(0), 0);
  const c1 = HUES[seed % HUES.length];
  const c2 = HUES[(seed + 2) % HUES.length];
  return /*#__PURE__*/React.createElement("span", {
    className: `vx-avatar ${className}`,
    style: {
      display: 'grid',
      placeItems: 'center',
      width: size,
      height: size,
      borderRadius: '50%',
      flexShrink: 0,
      overflow: 'hidden',
      color: '#fff',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: Math.round(size * 0.38),
      fontWeight: 700,
      background: src ? 'transparent' : `linear-gradient(135deg, ${c1}, ${c2})`,
      ...style
    }
  }, src ? /*#__PURE__*/React.createElement("img", {
    src: src,
    alt: name,
    style: {
      width: '100%',
      height: '100%',
      objectFit: 'cover'
    }
  }) : initials);
}
Object.assign(__ds_scope, { Switch, Avatar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/indicators/Switch.jsx", error: String((e && e.message) || e) }); }

// components/navigation/Tabs.jsx
try { (() => {
/**
 * Tabs — underline tab bar. `items` are { id, label, count }; controlled via
 * `value` / `onChange`.
 */
function Tabs({
  items = [],
  value,
  onChange,
  className = '',
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-tabs ${className}`,
    style: {
      display: 'flex',
      gap: 2,
      borderBottom: '1px solid var(--vx-line)',
      ...style
    }
  }, items.map(it => {
    const on = it.id === value;
    return /*#__PURE__*/React.createElement("button", {
      key: it.id,
      onClick: () => onChange?.(it.id),
      className: "vx-tab",
      style: {
        padding: '10px 12px',
        fontSize: 12,
        fontWeight: 500,
        marginBottom: -1,
        color: on ? 'var(--vx-ink)' : 'var(--vx-ink-3)',
        borderBottom: `2px solid ${on ? 'var(--vx-accent)' : 'transparent'}`,
        transition: 'color var(--vx-dur-fast)'
      }
    }, it.label, it.count != null && /*#__PURE__*/React.createElement("span", {
      style: {
        fontFamily: 'var(--vx-font-mono)',
        fontSize: 10,
        marginLeft: 5,
        color: 'var(--vx-ink-4)'
      }
    }, it.count));
  }));
}

/**
 * FilterChip — a toggleable filter token for filter bars. Shows key: value.
 */
function FilterChip({
  label,
  value,
  active,
  onClick,
  className = ''
}) {
  return /*#__PURE__*/React.createElement("button", {
    onClick: onClick,
    className: `vx-filter-chip ${className}`,
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      height: 24,
      padding: '0 8px',
      background: 'var(--vx-panel)',
      border: `1px solid ${active ? 'var(--vx-ink-3)' : 'var(--vx-line-2)'}`,
      borderRadius: 'var(--vx-radius-xs)',
      fontSize: 11,
      fontFamily: 'var(--vx-font-mono)',
      color: active ? 'var(--vx-ink)' : 'var(--vx-ink-2)',
      cursor: 'pointer',
      transition: 'border-color var(--vx-dur-fast)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-3)'
    }
  }, label), value && /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink)'
    }
  }, value));
}

/**
 * SidebarItem — a nav row for the app sidebar: icon, label, count, active rail.
 */
function SidebarItem({
  icon,
  label,
  count,
  active,
  onClick,
  className = ''
}) {
  return /*#__PURE__*/React.createElement("div", {
    onClick: onClick,
    className: `vx-side-item ${active ? 'vx-side-item-active' : ''} ${className}`,
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 8,
      padding: '5px 8px',
      minHeight: 26,
      borderRadius: 'var(--vx-radius-xs)',
      fontSize: 13,
      cursor: 'pointer',
      position: 'relative',
      color: active ? 'var(--vx-ink)' : 'var(--vx-ink-2)',
      background: active ? 'var(--vx-hl)' : 'transparent',
      fontWeight: active ? 500 : 400
    }
  }, active && /*#__PURE__*/React.createElement("span", {
    style: {
      position: 'absolute',
      left: -8,
      top: 4,
      bottom: 4,
      width: 2,
      background: 'var(--vx-accent)',
      borderRadius: 2
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      color: active ? 'var(--vx-accent)' : 'var(--vx-ink-3)',
      display: 'inline-flex',
      flexShrink: 0
    }
  }, icon), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1,
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap'
    }
  }, label), count != null && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)'
    }
  }, count), /*#__PURE__*/React.createElement("style", null, `.vx-side-item:hover { background: var(--vx-bg-2); color: var(--vx-ink); }`));
}
Object.assign(__ds_scope, { Tabs, FilterChip, SidebarItem });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/Tabs.jsx", error: String((e && e.message) || e) }); }

// components/surfaces/Card.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/** Card — the base surface: panel background, hairline border, soft radius. */
function Card({
  hover,
  glow,
  children,
  className = '',
  style,
  ...rest
}) {
  return /*#__PURE__*/React.createElement("div", _extends({
    className: `vx-card ${hover ? 'vx-card-hover' : ''} ${className}`,
    style: {
      background: 'var(--vx-panel)',
      border: '1px solid var(--vx-line)',
      borderRadius: 'var(--vx-radius-lg)',
      boxShadow: glow ? 'var(--vx-glow-soft)' : 'none',
      transition: 'border-color var(--vx-dur), transform var(--vx-dur)',
      ...style
    }
  }, rest), children, /*#__PURE__*/React.createElement("style", null, `.vx-card-hover:hover { border-color: var(--vx-line-3); }`));
}

/** Panel — a Card with a titled header row and padded body. */
function Panel({
  title,
  sub,
  actions,
  children,
  bodyStyle,
  className = '',
  style
}) {
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-panel ${className}`,
    style: {
      background: 'var(--vx-panel)',
      border: '1px solid var(--vx-line)',
      borderRadius: 'var(--vx-radius-lg)',
      overflow: 'hidden',
      ...style
    }
  }, (title || actions) && /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      padding: '10px 14px',
      borderBottom: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'baseline',
      gap: 10
    }
  }, title && /*#__PURE__*/React.createElement("span", {
    style: {
      fontWeight: 500,
      fontSize: 12,
      color: 'var(--vx-ink)'
    }
  }, title), sub && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)'
    }
  }, sub)), actions), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: 14,
      ...bodyStyle
    }
  }, children));
}
Object.assign(__ds_scope, { Card, Panel });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/surfaces/Card.jsx", error: String((e && e.message) || e) }); }

// components/surfaces/Stat.jsx
try { (() => {
/**
 * Sparkline — a tiny deterministic trend line (seeded, so it's stable across
 * renders). Colour follows `currentColor` / the `color` prop.
 */
function Sparkline({
  seed = 1,
  color = 'var(--vx-accent)',
  width = 100,
  height = 24,
  className = ''
}) {
  const n = 24;
  const pts = [];
  let v = 0.5;
  for (let i = 0; i < n; i++) {
    const r = Math.sin((seed + i) * 12.9898) * 43758.5453;
    v = Math.max(0.05, Math.min(0.95, v + (r - Math.floor(r) - 0.5) * 0.3));
    pts.push(v);
  }
  const path = pts.map((p, i) => `${i === 0 ? 'M' : 'L'} ${i / (n - 1) * width} ${height - p * height}`).join(' ');
  const area = `${path} L ${width} ${height} L 0 ${height} Z`;
  return /*#__PURE__*/React.createElement("svg", {
    className: `vx-spark ${className}`,
    viewBox: `0 0 ${width} ${height}`,
    preserveAspectRatio: "none",
    style: {
      color,
      width,
      height,
      display: 'block'
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: area,
    fill: "currentColor",
    opacity: "0.12"
  }), /*#__PURE__*/React.createElement("path", {
    d: path,
    stroke: "currentColor",
    fill: "none",
    strokeWidth: "1.25",
    vectorEffect: "non-scaling-stroke"
  }));
}
const DELTA = {
  up: {
    color: 'var(--vx-good)',
    arrow: '▲'
  },
  down: {
    color: 'var(--vx-err)',
    arrow: '▼'
  },
  flat: {
    color: 'var(--vx-ink-3)',
    arrow: '–'
  }
};

/**
 * Stat — a KPI cell: mono label, tabular value, optional delta + sparkline.
 * Lay several in a row with 1px dividers for a metrics strip.
 */
function Stat({
  label,
  value,
  delta,
  trend = 'flat',
  spark,
  seed = 1,
  className = '',
  style
}) {
  const d = DELTA[trend] || DELTA.flat;
  return /*#__PURE__*/React.createElement("div", {
    className: `vx-stat ${className}`,
    style: {
      padding: '14px 16px',
      display: 'flex',
      flexDirection: 'column',
      gap: 4,
      position: 'relative',
      minWidth: 0,
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      textTransform: 'uppercase',
      letterSpacing: '0.06em',
      color: 'var(--vx-ink-3)'
    }
  }, label), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 22,
      fontWeight: 500,
      letterSpacing: '-0.01em',
      fontVariantNumeric: 'tabular-nums',
      fontFamily: 'var(--vx-font-display)'
    }
  }, value), delta != null && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: d.color,
      display: 'inline-flex',
      alignItems: 'center',
      gap: 4
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 8
    }
  }, d.arrow), delta), spark && /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      right: 12,
      top: 14,
      width: 80,
      height: 28,
      opacity: 0.85
    }
  }, /*#__PURE__*/React.createElement(Sparkline, {
    seed: seed,
    color: d.color === 'var(--vx-ink-3)' ? 'var(--vx-accent)' : d.color,
    width: 80,
    height: 28
  })));
}
Object.assign(__ds_scope, { Sparkline, Stat });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/surfaces/Stat.jsx", error: String((e && e.message) || e) }); }

// ui_kits/app/app-kit.jsx
try { (() => {
// ============================================================
// Vortex App UI Kit — chat-first portal shell.
// Recreates the real product: topbar + sidebar + switchable
// screens (Chat, Knowledge bases, Models, Governance).
// Composes design-system primitives from window.Vortex_469485.
// ============================================================
const V = window.Vortex_469485;
const {
  PrismLogo,
  Wordmark,
  Icon,
  Button,
  IconButton,
  Badge,
  Tag,
  Switch,
  Avatar,
  Card,
  Panel,
  Stat,
  ModelChip,
  ProviderMark,
  DataTable,
  StatusPill,
  ToolChip,
  ChatMessage,
  ThinkBlock,
  ToolCard,
  Composer,
  Tabs,
  FilterChip,
  SidebarItem
} = V;

// ---- fake data ----
const CONVERSATIONS = [{
  id: 'c1',
  title: 'Q3 pricing guardrails',
  when: '2m',
  pinned: true,
  unread: true
}, {
  id: 'c2',
  title: 'Deal Desk policy rewrite',
  when: '1h'
}, {
  id: 'c3',
  title: 'Benchmark — Perplexity v Glean',
  when: '3h'
}, {
  id: 'c4',
  title: 'Onboarding email draft',
  when: 'Tue'
}, {
  id: 'c5',
  title: 'RAG eval golden set',
  when: 'Mon'
}];
const KBS = [{
  name: 'Product docs',
  count: 284
}, {
  name: 'Sales playbook',
  count: 41
}, {
  name: 'Engineering wiki',
  count: 1204
}, {
  name: 'HR policies',
  count: 62
}];
const MODELS = [['anthropic', 'Sonnet 4.6', 'high', '200k', '$3/$15', 'entitled'], ['openai', 'GPT-5', 'medium', '400k', '$5/$15', 'entitled'], ['google', 'Gemini 3 Pro', 'low', '1M', '$2/$10', 'preview'], ['mistral', 'Mistral Large', '', '128k', '$2/$6', 'disabled'], ['anthropic', 'Haiku 4', '', '200k', '$0.8/$4', 'entitled']];
const POLICIES = [['PII detection & redaction', 'guardrails_pii', 'block', true], ['Prompt injection protection', 'guardrails_prompt_injection', 'block', true], ['Secrets & credential leakage', 'guardrails_secrets', 'quarantine', true], ['Custom blocked terms', 'guardrails_custom_rules', 'warn', false], ['Moderation & topic bounds', 'guardrails_moderation', 'warn', true]];

// ---- Topbar ----
function Topbar({
  theme,
  setTheme
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      gridColumn: '1 / -1',
      display: 'flex',
      alignItems: 'center',
      height: 'var(--vx-topbar-h)',
      borderBottom: '1px solid var(--vx-line)',
      background: 'var(--vx-panel)',
      position: 'relative',
      zIndex: 5
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 'var(--vx-sidebar-w)',
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: '0 14px',
      height: '100%',
      borderRight: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement(PrismLogo, {
    state: "idle",
    size: 20
  }), /*#__PURE__*/React.createElement(Wordmark, {
    variant: "ink",
    size: 16
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      marginLeft: 'auto',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)',
      textTransform: 'uppercase',
      letterSpacing: '0.06em'
    }
  }, "prod")), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      maxWidth: 420,
      margin: '0 16px',
      display: 'flex',
      alignItems: 'center',
      gap: 8,
      height: 28,
      padding: '0 10px',
      background: 'var(--vx-bg-2)',
      border: '1px solid var(--vx-line-2)',
      borderRadius: 'var(--vx-radius-sm)',
      color: 'var(--vx-ink-3)'
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "search",
    size: 13
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      flex: 1,
      fontSize: 12
    }
  }, "Search conversations, knowledge bases, memories\u2026"), /*#__PURE__*/React.createElement("kbd", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)',
      padding: '1px 5px',
      border: '1px solid var(--vx-line-2)',
      borderRadius: 3
    }
  }, "\u2318K")), /*#__PURE__*/React.createElement("div", {
    style: {
      marginLeft: 'auto',
      display: 'flex',
      alignItems: 'center',
      gap: 12,
      padding: '0 16px'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 6,
      height: 24,
      padding: '0 8px',
      border: '1px solid var(--vx-line-2)',
      borderRadius: 3,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-2)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 6,
      height: 6,
      borderRadius: '50%',
      background: 'var(--vx-good)'
    }
  }), "spend \xB7 $4.2k / $8k"), /*#__PURE__*/React.createElement(IconButton, {
    title: "Theme",
    onClick: () => setTheme(theme === 'dark' ? 'light' : 'dark')
  }, /*#__PURE__*/React.createElement(Icon, {
    name: theme === 'dark' ? 'sun' : 'moon',
    size: 14
  })), /*#__PURE__*/React.createElement(IconButton, {
    title: "Notifications"
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "bell",
    size: 14
  })), /*#__PURE__*/React.createElement(Avatar, {
    name: "Dana Hollis",
    size: 26
  })));
}

// ---- Sidebar ----
function Sidebar({
  screen,
  setScreen,
  activeConv,
  setActiveConv
}) {
  const work = [{
    id: 'chat',
    label: 'Chat',
    icon: 'chat',
    count: CONVERSATIONS.length
  }, {
    id: 'kb',
    label: 'Knowledge bases',
    icon: 'library',
    count: KBS.length
  }, {
    id: 'memories',
    label: 'Memories',
    icon: 'brain',
    count: 14
  }];
  const admin = [{
    id: 'models',
    label: 'Models',
    icon: 'cpu',
    count: 4
  }, {
    id: 'keys',
    label: 'API keys',
    icon: 'key',
    count: 3
  }, {
    id: 'gov',
    label: 'Governance',
    icon: 'gov'
  }];
  return /*#__PURE__*/React.createElement("aside", {
    style: {
      background: 'var(--vx-panel)',
      borderRight: '1px solid var(--vx-line)',
      display: 'flex',
      flexDirection: 'column',
      overflow: 'hidden'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '10px 8px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "vx-label",
    style: {
      padding: '6px 8px'
    }
  }, "Workspace"), work.map(it => /*#__PURE__*/React.createElement(SidebarItem, {
    key: it.id,
    icon: /*#__PURE__*/React.createElement(Icon, {
      name: it.icon,
      size: 14
    }),
    label: it.label,
    count: it.count,
    active: screen === it.id,
    onClick: () => setScreen(it.id)
  }))), screen === 'chat' && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '2px 8px 10px',
      flex: 1,
      overflow: 'auto',
      borderTop: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "vx-label",
    style: {
      padding: '10px 8px 4px',
      display: 'flex',
      alignItems: 'center'
    }
  }, "Threads", /*#__PURE__*/React.createElement(IconButton, {
    size: "sm",
    title: "New",
    style: {
      marginLeft: 'auto'
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "plus",
    size: 11
  }))), CONVERSATIONS.map(c => /*#__PURE__*/React.createElement("div", {
    key: c.id,
    onClick: () => setActiveConv(c.id),
    className: "vx-conv",
    style: {
      padding: '8px 10px',
      borderRadius: 'var(--vx-radius-xs)',
      cursor: 'pointer',
      background: activeConv === c.id ? 'var(--vx-hl)' : 'transparent'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      fontWeight: 500,
      marginBottom: 3,
      color: 'var(--vx-ink)',
      display: 'flex',
      alignItems: 'center',
      gap: 6
    }
  }, c.pinned && /*#__PURE__*/React.createElement(Icon, {
    name: "pin",
    size: 10,
    style: {
      color: 'var(--vx-ink-3)'
    }
  }), c.title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)',
      display: 'flex',
      gap: 8,
      alignItems: 'center'
    }
  }, c.unread && /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: '50%',
      background: 'var(--vx-violet)'
    }
  }), c.when)))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '10px 8px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "vx-label",
    style: {
      padding: '6px 8px'
    }
  }, "Administer"), admin.map(it => /*#__PURE__*/React.createElement(SidebarItem, {
    key: it.id,
    icon: /*#__PURE__*/React.createElement(Icon, {
      name: it.icon,
      size: 14
    }),
    label: it.label,
    count: it.count,
    active: screen === it.id,
    onClick: () => setScreen(it.id)
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 'auto',
      padding: 12,
      borderTop: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 10,
      alignItems: 'center',
      padding: 8,
      border: '1px solid var(--vx-line-2)',
      borderRadius: 'var(--vx-radius-sm)',
      background: 'var(--vx-bg-2)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 22,
      height: 22,
      borderRadius: 4,
      background: 'linear-gradient(135deg, var(--vx-violet-deep), var(--vx-blue-deep))'
    }
  }), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontWeight: 500,
      fontSize: 12
    }
  }, "Northwind"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)'
    }
  }, "org_7f2a \xB7 412 members")))), /*#__PURE__*/React.createElement("style", null, `.vx-conv:hover { background: var(--vx-bg-2) !important; }`));
}

// ---- Chat screen ----
function ScreenChat() {
  const [draft, setDraft] = React.useState('');
  const [caps, setCaps] = React.useState({
    reflection: true,
    research: false
  });
  const conv = CONVERSATIONS[0];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      overflow: 'hidden',
      background: 'var(--vx-bg)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: '12px 18px',
      borderBottom: '1px solid var(--vx-line)',
      background: 'var(--vx-bg-2)'
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontWeight: 500,
      fontSize: 13
    }
  }, conv.title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)'
    }
  }, "Claude Sonnet 4.6 \xB7 Product docs + Sales playbook")), /*#__PURE__*/React.createElement("div", {
    style: {
      marginLeft: 'auto',
      display: 'flex',
      gap: 6
    }
  }, /*#__PURE__*/React.createElement(IconButton, {
    title: "Share"
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "link",
    size: 13
  })), /*#__PURE__*/React.createElement(IconButton, {
    title: "More"
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "more",
    size: 14
  })))), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflow: 'auto',
      padding: '22px 24px',
      display: 'flex',
      flexDirection: 'column',
      gap: 20
    }
  }, /*#__PURE__*/React.createElement(ChatMessage, {
    role: "user",
    who: "You",
    time: "2:13 PM"
  }, "What's our Q3 pricing guidance for deals over $250k?"), /*#__PURE__*/React.createElement(ChatMessage, {
    role: "ai",
    who: "Claude",
    time: "2:14 PM",
    state: "idle",
    grounded: true
  }, /*#__PURE__*/React.createElement(ThinkBlock, {
    meta: "2 tools \xB7 1.8s",
    done: true,
    defaultCollapsed: true
  }, /*#__PURE__*/React.createElement(ToolCard, {
    kind: "memory",
    name: "recall_memory",
    args: 'query: "Q3 pricing"',
    result: "14 facts",
    done: true
  }), /*#__PURE__*/React.createElement(ToolCard, {
    kind: "kb",
    name: "search_knowledge_base",
    args: "Product docs, Sales playbook",
    result: "3 chunks \xB7 0.91",
    done: true
  })), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: '0 0 10px'
    }
  }, "For Q3 2026, deals over ", /*#__PURE__*/React.createElement("strong", null, "$250k ARR"), " go through Deal Desk regardless of product mix. Standard packaging applies below that threshold, with two carve-outs:"), /*#__PURE__*/React.createElement("ul", {
    style: {
      margin: '0 0 10px',
      paddingLeft: 18,
      color: 'var(--vx-ink-2)'
    }
  }, /*#__PURE__*/React.createElement("li", null, "Multi-year terms"), /*#__PURE__*/React.createElement("li", null, "Land expansions above 40% of current ARR")), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: 0
    }
  }, "All quotes must include the 12-month default term and quarterly true-ups."), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 12,
      padding: '10px 12px',
      borderRadius: 6,
      background: 'var(--vx-tool-kb-bg)',
      border: '1px solid var(--vx-tool-kb-bd)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-tool-kb-fg)',
      textTransform: 'uppercase',
      letterSpacing: '0.08em',
      marginBottom: 6
    }
  }, "Sources"), [['Product docs · pricing-v12.md · §Deal Desk', '0.91'], ['Product docs · packaging.md · §Tiers', '0.84'], ['Sales playbook · gtm-q3.md · §3', '0.77']].map((s, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-2)',
      display: 'flex',
      justifyContent: 'space-between',
      padding: '2px 0'
    }
  }, /*#__PURE__*/React.createElement("span", null, s[0]), /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-tool-kb-fg)'
    }
  }, s[1])))))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '12px 24px',
      borderTop: '1px solid var(--vx-line)',
      background: 'var(--vx-bg-2)'
    }
  }, /*#__PURE__*/React.createElement(Composer, {
    value: draft,
    onChange: setDraft,
    capabilities: caps,
    onToggle: k => setCaps(c => ({
      ...c,
      [k]: !c[k]
    })),
    kbLabel: "2 KBs",
    model: {
      provider: 'anthropic',
      name: 'Sonnet 4.6',
      effort: 'high'
    }
  })));
}

// ---- Screen header ----
function Head({
  title,
  sub,
  actions
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'flex-end',
      justifyContent: 'space-between',
      padding: '20px 24px 16px',
      borderBottom: '1px solid var(--vx-line)',
      background: 'var(--vx-bg-2)'
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: 'var(--vx-font-display)',
      fontSize: 20,
      fontWeight: 600,
      letterSpacing: '-0.02em',
      margin: '0 0 4px'
    }
  }, title), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 12,
      color: 'var(--vx-ink-3)'
    }
  }, sub)), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 8
    }
  }, actions));
}

// ---- Models screen ----
function ScreenModels() {
  const [tab, setTab] = React.useState('all');
  const cols = [{
    label: 'Model'
  }, {
    label: 'Provider',
    mono: true
  }, {
    label: 'Context',
    align: 'right',
    mono: true
  }, {
    label: 'Cost in/out',
    align: 'right',
    mono: true
  }, {
    label: 'Status'
  }];
  const rows = MODELS.map(m => [/*#__PURE__*/React.createElement(ModelChip, {
    provider: m[0],
    name: m[1],
    effort: m[2],
    active: m[1] === 'Sonnet 4.6'
  }), m[0], m[3], m[4], /*#__PURE__*/React.createElement(StatusPill, {
    status: m[5]
  })]);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      overflow: 'auto',
      height: '100%'
    }
  }, /*#__PURE__*/React.createElement(Head, {
    title: "Models",
    sub: "catalog \xB7 entitlements \xB7 routing",
    actions: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(Button, {
      size: "sm",
      iconLeft: /*#__PURE__*/React.createElement(Icon, {
        name: "filter",
        size: 12
      })
    }, "Filter"), /*#__PURE__*/React.createElement(Button, {
      variant: "accent",
      size: "sm",
      iconLeft: /*#__PURE__*/React.createElement(Icon, {
        name: "plus",
        size: 12
      })
    }, "Add model"))
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: 'repeat(4,1fr)',
      borderBottom: '1px solid var(--vx-line)',
      background: 'var(--vx-panel)'
    }
  }, /*#__PURE__*/React.createElement(Stat, {
    label: "Entitled models",
    value: "12",
    delta: "3 new",
    trend: "up",
    spark: true,
    seed: 2,
    style: {
      borderRight: '1px solid var(--vx-line)'
    }
  }), /*#__PURE__*/React.createElement(Stat, {
    label: "Requests \xB7 7d",
    value: "18.4k",
    delta: "12%",
    trend: "up",
    spark: true,
    seed: 5,
    style: {
      borderRight: '1px solid var(--vx-line)'
    }
  }), /*#__PURE__*/React.createElement(Stat, {
    label: "Spend \xB7 7d",
    value: "$4.2k",
    delta: "8%",
    trend: "up",
    spark: true,
    seed: 9,
    style: {
      borderRight: '1px solid var(--vx-line)'
    }
  }), /*#__PURE__*/React.createElement(Stat, {
    label: "P50 first token",
    value: "0.7s",
    delta: "stable",
    trend: "flat",
    spark: true,
    seed: 12
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '12px 16px 0'
    }
  }, /*#__PURE__*/React.createElement(Tabs, {
    value: tab,
    onChange: setTab,
    items: [{
      id: 'all',
      label: 'All',
      count: 12
    }, {
      id: 'entitled',
      label: 'Entitled',
      count: 8
    }, {
      id: 'preview',
      label: 'Preview',
      count: 4
    }]
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: 16
    }
  }, /*#__PURE__*/React.createElement(Card, null, /*#__PURE__*/React.createElement(DataTable, {
    columns: cols,
    rows: rows,
    onRowClick: () => {}
  }))));
}

// ---- Governance screen ----
function ScreenGov() {
  const [policies, setPolicies] = React.useState(POLICIES.map(p => p[3]));
  return /*#__PURE__*/React.createElement("div", {
    style: {
      overflow: 'auto',
      height: '100%'
    }
  }, /*#__PURE__*/React.createElement(Head, {
    title: "Governance",
    sub: "guardrails \xB7 policies \xB7 audit",
    actions: /*#__PURE__*/React.createElement(Button, {
      variant: "accent",
      size: "sm",
      iconLeft: /*#__PURE__*/React.createElement(Icon, {
        name: "plus",
        size: 12
      })
    }, "New rule")
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: 16,
      display: 'grid',
      gridTemplateColumns: '1.4fr 1fr',
      gap: 12
    }
  }, /*#__PURE__*/React.createElement(Panel, {
    title: "Guardrail policies",
    sub: "applied to every assistant & route"
  }, POLICIES.map((p, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      display: 'grid',
      gridTemplateColumns: '1fr 130px 90px 40px',
      gap: 12,
      padding: '10px 2px',
      borderBottom: i < POLICIES.length - 1 ? '1px solid var(--vx-line-2)' : 'none',
      alignItems: 'center'
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      fontWeight: 500,
      fontSize: 13
    }
  }, p[0]), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      color: 'var(--vx-ink-3)'
    }
  }, p[1])), /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(Badge, {
    tone: p[2] === 'block' ? 'err' : p[2] === 'quarantine' ? 'warn' : 'neutral'
  }, p[2])), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)'
    }
  }, policies[i] ? 'active' : 'off'), /*#__PURE__*/React.createElement(Switch, {
    checked: policies[i],
    onChange: v => setPolicies(ps => ps.map((x, j) => j === i ? v : x))
  })))), /*#__PURE__*/React.createElement(Panel, {
    title: "Recent audit",
    sub: "last 24h"
  }, [['pii.redact', 'blocked', 'err', '2m'], ['injection.detect', 'flagged', 'warn', '18m'], ['policy.update', 'custom_rules', 'accent', '1h'], ['secrets.scan', 'clean', 'ok', '2h'], ['pii.redact', 'blocked', 'err', '3h']].map((a, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      display: 'grid',
      gridTemplateColumns: '1fr auto auto',
      gap: 10,
      alignItems: 'center',
      padding: '7px 2px',
      borderBottom: i < 4 ? '1px solid var(--vx-line-2)' : 'none',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-2)'
    }
  }, a[0]), /*#__PURE__*/React.createElement(Badge, {
    tone: a[2],
    dot: a[2] !== 'accent'
  }, a[1]), /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-3)'
    }
  }, a[3]))))));
}

// ---- KB screen ----
function ScreenKB() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      overflow: 'auto',
      height: '100%'
    }
  }, /*#__PURE__*/React.createElement(Head, {
    title: "Knowledge bases",
    sub: "hybrid BM25 + pgvector + rerank",
    actions: /*#__PURE__*/React.createElement(Button, {
      variant: "accent",
      size: "sm",
      iconLeft: /*#__PURE__*/React.createElement(Icon, {
        name: "upload",
        size: 12
      })
    }, "Upload")
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: 16,
      display: 'grid',
      gridTemplateColumns: 'repeat(3,1fr)',
      gap: 12
    }
  }, KBS.map((kb, i) => /*#__PURE__*/React.createElement(Card, {
    key: i,
    hover: true,
    style: {
      padding: 16
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'flex-start',
      justifyContent: 'space-between'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 30,
      height: 30,
      borderRadius: 6,
      background: 'var(--vx-tool-kb-bg)',
      color: 'var(--vx-tool-kb-fg)',
      display: 'grid',
      placeItems: 'center'
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "library",
    size: 15
  })), /*#__PURE__*/React.createElement(StatusPill, {
    status: i === 2 ? 'indexing' : 'ready'
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      fontWeight: 600,
      fontSize: 14,
      marginTop: 12
    }
  }, kb.name), /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)',
      marginTop: 2
    }
  }, kb.count.toLocaleString(), " documents"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6,
      marginTop: 12,
      paddingTop: 12,
      borderTop: '1px solid var(--vx-line-2)'
    }
  }, /*#__PURE__*/React.createElement(Tag, null, "voyage-4-lite"), /*#__PURE__*/React.createElement(Tag, null, "top_k=8"))))));
}
function ScreenStub({
  title,
  sub
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      height: '100%'
    }
  }, /*#__PURE__*/React.createElement(Head, {
    title: title,
    sub: sub
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: 40,
      color: 'var(--vx-ink-3)',
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 12
    }
  }, "\u2014 screen recreation available on request \u2014"));
}

// ---- App root ----
function App() {
  const [theme, setTheme] = React.useState('light');
  const [screen, setScreen] = React.useState('chat');
  const [activeConv, setActiveConv] = React.useState('c1');
  React.useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: 'var(--vx-sidebar-w) 1fr',
      gridTemplateRows: 'var(--vx-topbar-h) 1fr',
      height: '100vh'
    }
  }, /*#__PURE__*/React.createElement(Topbar, {
    theme: theme,
    setTheme: setTheme
  }), /*#__PURE__*/React.createElement(Sidebar, {
    screen: screen,
    setScreen: setScreen,
    activeConv: activeConv,
    setActiveConv: setActiveConv
  }), /*#__PURE__*/React.createElement("main", {
    style: {
      overflow: 'hidden'
    }
  }, screen === 'chat' && /*#__PURE__*/React.createElement(ScreenChat, null), screen === 'kb' && /*#__PURE__*/React.createElement(ScreenKB, null), screen === 'models' && /*#__PURE__*/React.createElement(ScreenModels, null), screen === 'gov' && /*#__PURE__*/React.createElement(ScreenGov, null), screen === 'memories' && /*#__PURE__*/React.createElement(ScreenStub, {
    title: "Memories",
    sub: "14 active \xB7 preferences \xB7 context \xB7 tools"
  }), screen === 'keys' && /*#__PURE__*/React.createElement(ScreenStub, {
    title: "API keys",
    sub: "portal keys \xB7 aip_\u2026"
  })));
}
if (typeof ReactDOM !== 'undefined' && document.getElementById('root')) {
  ReactDOM.createRoot(document.getElementById('root')).render(/*#__PURE__*/React.createElement(App, null));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/app/app-kit.jsx", error: String((e && e.message) || e) }); }

// ui_kits/marketing/marketing-kit.jsx
try { (() => {
// ============================================================
// Vortex Marketing UI Kit — public landing page.
// Dark stage, spectrum brand, Prism mark. Composes DS primitives.
// ============================================================
const V = window.Vortex_469485;
const {
  PrismLogo,
  Wordmark,
  BrandLockup,
  Button,
  Icon,
  Badge,
  Tag,
  Stat,
  ChatMessage,
  ThinkBlock,
  ToolCard,
  Composer,
  ModelChip
} = V;
function Nav() {
  return /*#__PURE__*/React.createElement("nav", {
    style: {
      position: 'sticky',
      top: 0,
      zIndex: 20,
      backdropFilter: 'blur(14px)',
      background: 'rgba(4,4,7,0.72)',
      borderBottom: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '14px 32px',
      display: 'flex',
      alignItems: 'center',
      gap: 28
    }
  }, /*#__PURE__*/React.createElement(BrandLockup, {
    size: 26
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 26,
      fontSize: 14,
      color: 'var(--vx-ink-2)',
      marginLeft: 8
    }
  }, /*#__PURE__*/React.createElement("a", {
    href: "#how"
  }, "Features"), /*#__PURE__*/React.createElement("a", {
    href: "#how"
  }, "How it works"), /*#__PURE__*/React.createElement("a", {
    href: "#"
  }, "Docs"), /*#__PURE__*/React.createElement("a", {
    href: "#"
  }, "Blog")), /*#__PURE__*/React.createElement("div", {
    style: {
      marginLeft: 'auto',
      display: 'flex',
      gap: 8
    }
  }, /*#__PURE__*/React.createElement(Button, {
    variant: "ghost"
  }, "Sign in"), /*#__PURE__*/React.createElement(Button, {
    gradient: true,
    iconRight: /*#__PURE__*/React.createElement(Icon, {
      name: "arrow-right",
      size: 13
    })
  }, "Get started"))));
}
function Hero() {
  const [caps, setCaps] = React.useState({
    reflection: true,
    research: true
  });
  return /*#__PURE__*/React.createElement("section", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '72px 32px 80px',
      display: 'grid',
      gridTemplateColumns: '1fr 1.05fr',
      gap: 56,
      alignItems: 'center'
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 8,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-2)',
      textTransform: 'uppercase',
      letterSpacing: '0.1em',
      marginBottom: 26
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 24,
      height: 1,
      background: 'var(--vx-violet)'
    }
  }), "AI Portal \xB7 Built for teams"), /*#__PURE__*/React.createElement("h1", {
    style: {
      fontFamily: 'var(--vx-font-display)',
      margin: '0 0 26px',
      fontSize: 64,
      fontWeight: 700,
      letterSpacing: '-0.035em',
      lineHeight: 1.0
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'block',
      color: 'var(--vx-ink)'
    }
  }, "Ask anything."), /*#__PURE__*/React.createElement("span", {
    className: "vx-grad-text",
    style: {
      display: 'block'
    }
  }, "Know everything."), /*#__PURE__*/React.createElement("span", {
    style: {
      display: 'block',
      color: 'var(--vx-ink-3)',
      fontWeight: 500
    }
  }, "Ship faster.")), /*#__PURE__*/React.createElement("p", {
    style: {
      fontSize: 18,
      lineHeight: 1.55,
      color: 'var(--vx-ink-2)',
      maxWidth: 500,
      margin: '0 0 32px'
    }
  }, "Vortex is the AI portal your team actually wants to use. One chat for every model. Your knowledge, your memory, your guardrails \u2014 under one roof."), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 10,
      marginBottom: 30
    }
  }, /*#__PURE__*/React.createElement(Button, {
    gradient: true,
    size: "lg",
    iconRight: /*#__PURE__*/React.createElement(Icon, {
      name: "arrow-right",
      size: 14
    })
  }, "Start for free"), /*#__PURE__*/React.createElement(Button, {
    size: "lg"
  }, "How it works")), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 12,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)'
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: '50%',
      background: 'var(--vx-good)',
      boxShadow: '0 0 10px var(--vx-good)'
    }
  }), "No credit card \xB7 Google, GitHub, or email \xB7 Self-host ready")), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      borderRadius: 'var(--vx-radius-xl)',
      overflow: 'hidden',
      background: 'var(--vx-bg-2)',
      border: '1px solid var(--vx-line)',
      boxShadow: '0 1px 0 rgba(255,255,255,0.04) inset, 0 40px 80px -30px rgba(0,0,0,0.6), var(--vx-glow-soft)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      alignItems: 'center',
      gap: 10,
      padding: '10px 14px',
      borderBottom: '1px solid var(--vx-line)',
      background: 'var(--vx-bg-2)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 6
    }
  }, [0, 1, 2].map(i => /*#__PURE__*/React.createElement("span", {
    key: i,
    style: {
      width: 10,
      height: 10,
      borderRadius: '50%',
      background: 'var(--vx-line-2)'
    }
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      margin: '0 8px',
      height: 22,
      borderRadius: 5,
      background: 'var(--vx-bg-3)',
      border: '1px solid var(--vx-line)',
      padding: '0 10px',
      display: 'flex',
      alignItems: 'center',
      gap: 6,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)'
    }
  }, /*#__PURE__*/React.createElement(Icon, {
    name: "lock",
    size: 9
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-ink-2)'
    }
  }, "vortex.app"), "/chat/c/0x9f4a\u2026")), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: '18px 18px 14px',
      display: 'flex',
      flexDirection: 'column',
      gap: 16,
      minHeight: 320
    }
  }, /*#__PURE__*/React.createElement(ChatMessage, {
    role: "user",
    who: "You",
    time: "now"
  }, "What's our Q3 pricing guidance for deals over $250k?"), /*#__PURE__*/React.createElement(ChatMessage, {
    role: "ai",
    who: "Claude",
    time: "now",
    state: "streaming",
    grounded: true
  }, /*#__PURE__*/React.createElement(ThinkBlock, {
    meta: "2 tools \xB7 1.8s",
    done: true,
    defaultCollapsed: true
  }, /*#__PURE__*/React.createElement(ToolCard, {
    kind: "memory",
    name: "recall_memory",
    result: "14 facts",
    done: true
  }), /*#__PURE__*/React.createElement(ToolCard, {
    kind: "kb",
    name: "search_knowledge_base",
    result: "3 chunks \xB7 0.91",
    done: true
  })), "For Q3, deals over $250k route to Deal Desk regardless of product mix \u2014 with carve-outs for multi-year terms and land expansions above 40%."), /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 'auto'
    }
  }, /*#__PURE__*/React.createElement(Composer, {
    capabilities: caps,
    onToggle: k => setCaps(c => ({
      ...c,
      [k]: !c[k]
    })),
    kbLabel: "2 KBs",
    model: {
      provider: 'anthropic',
      name: 'Sonnet 4.6',
      effort: 'high'
    }
  })))));
}
function LogoBand() {
  const names = ['Northwind', 'Kestrel Labs', 'Meridian', 'Halcyon', 'Runway Systems', 'Basecase', 'Atlas Robotics', 'Vellum'];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '40px 32px',
      borderTop: '1px solid var(--vx-line)',
      borderBottom: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)',
      textAlign: 'center',
      textTransform: 'uppercase',
      letterSpacing: '0.15em',
      marginBottom: 22
    }
  }, "Teams shipping with Vortex"), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 48,
      justifyContent: 'center',
      flexWrap: 'wrap'
    }
  }, names.map(n => /*#__PURE__*/React.createElement("span", {
    key: n,
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontWeight: 600,
      fontSize: 18,
      color: 'var(--vx-ink-3)',
      display: 'inline-flex',
      alignItems: 'center',
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 5,
      height: 5,
      borderRadius: '50%',
      background: 'var(--vx-line-3)'
    }
  }), n))));
}
const FEATURES = [['01 · CHAT', 'pink', 'Streaming from turn one.', 'First byte under a second. Resume, stop, regenerate. Markdown, code, tables. Every message addressable by URL.'], ['02 · KNOWLEDGE', 'violet', 'Hybrid retrieval that works.', 'BM25 + pgvector + reranking. Scoped per-conversation. Citations that point to the paragraph, not the file.'], ['03 · IDENTITY', 'blue', 'Sign in the way you want.', 'Google, GitHub, or magic link for teams. Microsoft Entra, OIDC, SAML for enterprise. One identity model.'], ['04 · GUARDRAILS', 'pink', 'Policies that block.', 'PII detection, prompt injection, secrets, custom rules. Per-tenant, per-assistant, audit-logged.'], ['05 · OBSERVABILITY', 'violet', 'Trace every turn.', 'Every run is a tree. See the prompt, retrieval, tool calls, cost. Replay against a new model in one click.'], ['06 · SELF-HOST', 'blue', 'Your infra, your data.', 'Single Docker Compose. Postgres + pgvector + Redis. Deploy to Render, Azure, bare metal. BYO-keys.']];
function Features() {
  const colors = {
    pink: 'var(--vx-pink)',
    violet: 'var(--vx-violet)',
    blue: 'var(--vx-blue)'
  };
  return /*#__PURE__*/React.createElement("section", {
    id: "how",
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '110px 32px'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 48
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'inline-flex',
      alignItems: 'center',
      gap: 8,
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-violet)',
      textTransform: 'uppercase',
      letterSpacing: '0.12em',
      marginBottom: 18
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      width: 24,
      height: 1,
      background: 'var(--vx-violet)'
    }
  }), "Under the hood"), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: 'var(--vx-font-display)',
      margin: 0,
      fontSize: 44,
      fontWeight: 600,
      letterSpacing: '-0.028em',
      lineHeight: 1.05,
      maxWidth: 760
    }
  }, "A real product, not a ", /*#__PURE__*/React.createElement("span", {
    className: "vx-grad-text"
  }, "prompt wrapper."))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'grid',
      gridTemplateColumns: 'repeat(3,1fr)',
      gap: 20
    }
  }, FEATURES.map((f, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      border: '1px solid var(--vx-line)',
      borderRadius: 'var(--vx-radius-lg)',
      padding: 26,
      background: 'var(--vx-bg-2)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 12,
      color: colors[f[1]],
      marginBottom: 16
    }
  }, "\u2014 ", f[0]), /*#__PURE__*/React.createElement("h3", {
    style: {
      margin: '0 0 10px',
      fontSize: 21,
      fontWeight: 600,
      letterSpacing: '-0.02em'
    }
  }, f[2]), /*#__PURE__*/React.createElement("p", {
    style: {
      color: 'var(--vx-ink-2)',
      margin: 0,
      fontSize: 14,
      lineHeight: 1.6
    }
  }, f[3])))));
}
function Stats() {
  const items = [['Models', '10+', 'Anthropic · OpenAI · Google · Mistral · OSS'], ['KB size', '∞', 'Millions of docs. pgvector + rerank.'], ['Self-host', '100%', 'One Docker Compose. Your data stays.'], ['First token', '<1s', 'Streaming from turn one.']];
  return /*#__PURE__*/React.createElement("section", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '70px 32px',
      display: 'grid',
      gridTemplateColumns: 'repeat(4,1fr)',
      gap: 32,
      borderTop: '1px solid var(--vx-line)',
      borderBottom: '1px solid var(--vx-line)'
    }
  }, items.map((s, i) => /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      paddingRight: 20,
      borderRight: i < 3 ? '1px solid var(--vx-line)' : 'none'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 11,
      color: 'var(--vx-ink-3)',
      textTransform: 'uppercase',
      letterSpacing: '0.12em',
      marginBottom: 12
    }
  }, s[0]), /*#__PURE__*/React.createElement("div", {
    className: "vx-grad-text",
    style: {
      fontFamily: 'var(--vx-font-display)',
      fontSize: 52,
      fontWeight: 600,
      letterSpacing: '-0.03em',
      lineHeight: 1,
      marginBottom: 10
    }
  }, s[1]), /*#__PURE__*/React.createElement("div", {
    style: {
      color: 'var(--vx-ink-2)',
      fontSize: 13
    }
  }, s[2]))));
}
function Mission() {
  return /*#__PURE__*/React.createElement("section", {
    style: {
      maxWidth: 860,
      margin: '0 auto',
      padding: '120px 32px',
      textAlign: 'center'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      marginBottom: 30
    }
  }, /*#__PURE__*/React.createElement(PrismLogo, {
    state: "idle",
    size: 72
  })), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: 'var(--vx-font-display)',
      margin: '0 0 22px',
      fontSize: 38,
      fontWeight: 500,
      letterSpacing: '-0.025em',
      lineHeight: 1.2
    }
  }, "We believe every team should be able to ", /*#__PURE__*/React.createElement("span", {
    className: "vx-grad-text"
  }, "talk to their work"), " \u2014 without handing their data to a black box."), /*#__PURE__*/React.createElement("p", {
    style: {
      color: 'var(--vx-ink-2)',
      fontSize: 17,
      lineHeight: 1.6,
      margin: 0
    }
  }, "Vortex is open-source at the core, self-hostable end to end, and built for teams that take their own context seriously."));
}
function CTA() {
  return /*#__PURE__*/React.createElement("section", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '120px 32px',
      textAlign: 'center',
      position: 'relative',
      overflow: 'hidden'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'absolute',
      left: '50%',
      bottom: -200,
      transform: 'translateX(-50%)',
      width: 900,
      height: 500,
      background: 'radial-gradient(ellipse at center, rgba(var(--vx-violet-rgb),0.3), transparent 60%)',
      pointerEvents: 'none',
      filter: 'blur(20px)'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      justifyContent: 'center',
      marginBottom: 28
    }
  }, /*#__PURE__*/React.createElement(PrismLogo, {
    state: "streaming",
    size: 72
  })), /*#__PURE__*/React.createElement("h2", {
    style: {
      fontFamily: 'var(--vx-font-display)',
      margin: '0 0 18px',
      fontSize: 56,
      fontWeight: 600,
      letterSpacing: '-0.03em',
      lineHeight: 1.02
    }
  }, "Ask anything. ", /*#__PURE__*/React.createElement("span", {
    className: "vx-grad-text"
  }, "Know everything.")), /*#__PURE__*/React.createElement("p", {
    style: {
      fontSize: 18,
      color: 'var(--vx-ink-2)',
      marginBottom: 30
    }
  }, "Join the public beta. Sign in with Google, GitHub, or email. It takes about 12 seconds."), /*#__PURE__*/React.createElement("div", {
    style: {
      display: 'flex',
      gap: 10,
      justifyContent: 'center'
    }
  }, /*#__PURE__*/React.createElement(Button, {
    gradient: true,
    size: "lg",
    iconRight: /*#__PURE__*/React.createElement(Icon, {
      name: "arrow-right",
      size: 14
    })
  }, "Start for free"), /*#__PURE__*/React.createElement(Button, {
    size: "lg"
  }, "Read the docs"))));
}
function Footer() {
  const cols = [['Product', ['Chat', 'Knowledge', 'Memories', 'Governance']], ['Developers', ['Docs', 'API', 'Changelog', 'GitHub']], ['Company', ['About', 'Security', 'Careers']], ['Legal', ['Privacy', 'Terms', 'DPA']]];
  return /*#__PURE__*/React.createElement("footer", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '56px 32px 40px',
      display: 'grid',
      gridTemplateColumns: '1.6fr 1fr 1fr 1fr 1fr',
      gap: 40,
      borderTop: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", null, /*#__PURE__*/React.createElement(BrandLockup, {
    size: 22
  }), /*#__PURE__*/React.createElement("p", {
    style: {
      maxWidth: 280,
      color: 'var(--vx-ink-3)',
      marginTop: 12,
      fontSize: 13,
      lineHeight: 1.55
    }
  }, "The AI portal your team actually wants to use. Open-source. Self-hostable. Enterprise-ready.")), cols.map(([h, links]) => /*#__PURE__*/React.createElement("div", {
    key: h
  }, /*#__PURE__*/React.createElement("h4", {
    style: {
      fontFamily: 'var(--vx-font-mono)',
      fontSize: 10,
      textTransform: 'uppercase',
      letterSpacing: '0.1em',
      color: 'var(--vx-ink-3)',
      margin: '0 0 14px'
    }
  }, h), /*#__PURE__*/React.createElement("ul", {
    style: {
      listStyle: 'none',
      padding: 0,
      margin: 0,
      display: 'flex',
      flexDirection: 'column',
      gap: 10
    }
  }, links.map(l => /*#__PURE__*/React.createElement("li", {
    key: l
  }, /*#__PURE__*/React.createElement("a", {
    href: "#",
    style: {
      color: 'var(--vx-ink-2)',
      fontSize: 13
    }
  }, l)))))));
}
function Landing() {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative'
    }
  }, /*#__PURE__*/React.createElement("div", {
    className: "vx-atmosphere",
    style: {
      position: 'absolute'
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      position: 'relative',
      zIndex: 1
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      background: 'var(--vx-grad-soft)',
      borderBottom: '1px solid var(--vx-line)'
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 1180,
      margin: '0 auto',
      padding: '9px 32px',
      display: 'flex',
      alignItems: 'center',
      gap: 12,
      justifyContent: 'center',
      fontSize: 13,
      color: 'var(--vx-ink-2)'
    }
  }, /*#__PURE__*/React.createElement(Badge, {
    tone: "pink"
  }, "beta"), " Vortex is open for public beta \u2014 Google, GitHub, or email to get in. ", /*#__PURE__*/React.createElement("span", {
    style: {
      color: 'var(--vx-violet)'
    }
  }, "\u2192"))), /*#__PURE__*/React.createElement(Nav, null), /*#__PURE__*/React.createElement(Hero, null), /*#__PURE__*/React.createElement(LogoBand, null), /*#__PURE__*/React.createElement(Features, null), /*#__PURE__*/React.createElement(Stats, null), /*#__PURE__*/React.createElement(Mission, null), /*#__PURE__*/React.createElement(CTA, null), /*#__PURE__*/React.createElement(Footer, null)));
}
if (typeof ReactDOM !== 'undefined' && document.getElementById('root')) {
  ReactDOM.createRoot(document.getElementById('root')).render(/*#__PURE__*/React.createElement(Landing, null));
}
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/marketing/marketing-kit.jsx", error: String((e && e.message) || e) }); }

__ds_ns.PrismLogo = __ds_scope.PrismLogo;

__ds_ns.Wordmark = __ds_scope.Wordmark;

__ds_ns.BrandLockup = __ds_scope.BrandLockup;

__ds_ns.ChatMessage = __ds_scope.ChatMessage;

__ds_ns.ThinkBlock = __ds_scope.ThinkBlock;

__ds_ns.ToolCard = __ds_scope.ToolCard;

__ds_ns.Composer = __ds_scope.Composer;

__ds_ns.Button = __ds_scope.Button;

__ds_ns.IconButton = __ds_scope.IconButton;

__ds_ns.Icon = __ds_scope.Icon;

__ds_ns.DataTable = __ds_scope.DataTable;

__ds_ns.ProviderMark = __ds_scope.ProviderMark;

__ds_ns.ProviderChip = __ds_scope.ProviderChip;

__ds_ns.ModelChip = __ds_scope.ModelChip;

__ds_ns.StatusPill = __ds_scope.StatusPill;

__ds_ns.ThinkingDots = __ds_scope.ThinkingDots;

__ds_ns.ToolChip = __ds_scope.ToolChip;

__ds_ns.Field = __ds_scope.Field;

__ds_ns.Input = __ds_scope.Input;

__ds_ns.Textarea = __ds_scope.Textarea;

__ds_ns.Select = __ds_scope.Select;

__ds_ns.Checkbox = __ds_scope.Checkbox;

__ds_ns.Badge = __ds_scope.Badge;

__ds_ns.Tag = __ds_scope.Tag;

__ds_ns.Switch = __ds_scope.Switch;

__ds_ns.Avatar = __ds_scope.Avatar;

__ds_ns.Tabs = __ds_scope.Tabs;

__ds_ns.FilterChip = __ds_scope.FilterChip;

__ds_ns.SidebarItem = __ds_scope.SidebarItem;

__ds_ns.Card = __ds_scope.Card;

__ds_ns.Panel = __ds_scope.Panel;

__ds_ns.Sparkline = __ds_scope.Sparkline;

__ds_ns.Stat = __ds_scope.Stat;

})();
