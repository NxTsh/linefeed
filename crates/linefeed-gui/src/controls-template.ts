// The overlay chrome as a pure string so tests can pin what ships without a
// DOM. Every actionable element carries a data-c id; controls.ts is the ONLY
// module that reads them.

export const CONTROLS_TEMPLATE = `
<div id="controls">
  <div class="group">
    <button data-c="open" title="Open script (Cmd/Ctrl+O)">Open</button>
  </div>
  <div class="sep"></div>
  <div class="group">
    <button data-c="mode" title="Toggle voice / dumb scroll">voice</button>
    <button data-c="start" title="Start listening">Listen</button>
    <button data-c="play" class="hidden" title="Play/pause (Space)">▶</button>
    <span  data-c="wpm-group" class="hidden">
      <button data-c="wpm-down" title="Slower">−</button>
      <span class="value" data-c="wpm">140</span>
      <button data-c="wpm-up" title="Faster">+</button>
    </span>
  </div>
  <div class="sep"></div>
  <div class="group">
    <button data-c="font-down" title="Smaller text">A−</button>
    <span class="value" data-c="font-px">56</span>
    <button data-c="font-up" title="Bigger text">A+</button>
    <select data-c="reading-font" title="Reading font"></select>
  </div>
  <div class="sep"></div>
  <div class="group">
    <button data-c="zone-w-down" title="Zone narrower (Alt+←)">W−</button>
    <button data-c="zone-w-up" title="Zone wider (Alt+→)">W+</button>
    <button data-c="zone-h-down" title="Zone shorter (Alt+↓)">H−</button>
    <button data-c="zone-h-up" title="Zone taller (Alt+↑)">H+</button>
    <button data-c="lead-down" title="Lookahead down ([)">L−</button>
    <span class="value" data-c="lead">1</span>
    <button data-c="lead-up" title="Lookahead up (])">L+</button>
  </div>
  <div class="sep"></div>
  <div class="group">
    <button data-c="mirror-h" title="Mirror horizontally">⇄</button>
    <button data-c="mirror-v" title="Mirror vertically">⇅</button>
    <button data-c="fullscreen" title="Fullscreen (F11 / Cmd+F)">⛶</button>
    <button data-c="present" title="Presentation mode (h)">👁</button>
    <button data-c="resume" class="hidden" title="Resume follow (f)">follow</button>
  </div>
</div>
<div id="pill"><span class="dot"></span><span class="label" data-c="pill-label">idle</span></div>
<div id="posbar"></div>
<div id="banner">re-syncing…</div>
`;
