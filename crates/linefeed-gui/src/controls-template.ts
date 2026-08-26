// The overlay chrome as a pure string so tests can pin what ships without a
// DOM. Every actionable element carries a data-c id; controls.ts is the ONLY
// module that reads them.
//
// Layout philosophy: the BOTTOM BAR carries only what you touch during a
// session (open, device, listen, mode, transport). Everything you set up
// once lives in the SETTINGS PANEL behind the gear, in labeled sections.

export const CONTROLS_TEMPLATE = `
<div id="controls">
  <div class="group">
    <button data-c="open" title="Open script (Cmd/Ctrl+O)">Open</button>
  </div>
  <div class="sep"></div>
  <div class="group">
    <button data-c="mode" title="Toggle voice / dumb scroll">voice</button>
    <select data-c="device" title="Input device (empty = system default)"></select>
    <button data-c="start" class="primary" title="Start listening">Listen</button>
    <button data-c="play" class="primary hidden" title="Play/pause (Space)">▶</button>
    <span class="group hidden" data-c="wpm-group">
      <button data-c="wpm-down" title="Slower">−</button>
      <span class="value" data-c="wpm">140</span>
      <button data-c="wpm-up" title="Faster">+</button>
    </span>
  </div>
  <div class="sep"></div>
  <div class="group">
    <button data-c="resume" class="hidden" title="Resume follow (f)">follow</button>
    <button data-c="fullscreen" title="Fullscreen (F11 / Cmd+F)">⛶</button>
    <button data-c="present" title="Presentation mode (h)">👁</button>
    <button data-c="settings" title="Settings">⚙</button>
  </div>
</div>

<div id="settings-panel" class="hidden">
  <div class="panel-section">
    <h3>Reading</h3>
    <div class="row">
      <span class="label">Text size</span>
      <span class="cluster">
        <button data-c="font-down" title="Smaller text">A−</button>
        <span class="value" data-c="font-px">56</span>
        <button data-c="font-up" title="Bigger text">A+</button>
      </span>
    </div>
    <div class="row">
      <span class="label">Font</span>
      <select data-c="reading-font" title="Reading font"></select>
    </div>
    <div class="row">
      <span class="label">Lookahead</span>
      <span class="cluster">
        <button data-c="lead-down" title="Fewer lines ([)">−</button>
        <span class="value" data-c="lead">1</span>
        <button data-c="lead-up" title="More lines (])">+</button>
        <span class="unit">lines</span>
      </span>
    </div>
  </div>
  <div class="panel-section">
    <h3>Layout</h3>
    <div class="row">
      <span class="label">Zone width</span>
      <span class="cluster">
        <button data-c="zone-w-down" title="Narrower (Alt+←)">−</button>
        <span class="value" data-c="zone-w">90</span>
        <button data-c="zone-w-up" title="Wider (Alt+→)">+</button>
        <span class="unit">%</span>
      </span>
    </div>
    <div class="row">
      <span class="label">Zone height</span>
      <span class="cluster">
        <button data-c="zone-h-down" title="Shorter (Alt+↓)">−</button>
        <span class="value" data-c="zone-h">80</span>
        <button data-c="zone-h-up" title="Taller (Alt+↑)">+</button>
        <span class="unit">%</span>
      </span>
    </div>
    <div class="row">
      <span class="label">Mirror</span>
      <span class="cluster">
        <button data-c="mirror-h" title="Mirror horizontally (beam-splitter)">⇄ H</button>
        <button data-c="mirror-v" title="Mirror vertically">⇅ V</button>
      </span>
    </div>
  </div>
  <div class="panel-section">
    <h3>Voice model</h3>
    <div class="row">
      <span class="label">Language</span>
      <select data-c="model" title="ASR model used for voice-following"></select>
    </div>
    <div data-c="model-list" class="model-list"></div>
  </div>
  <div class="panel-section shortcuts">
    <h3>Keys</h3>
    <div class="keys">
      <span><kbd>h</kbd> hide chrome</span>
      <span><kbd>f</kbd> resume follow</span>
      <span><kbd>[</kbd><kbd>]</kbd> lookahead</span>
      <span><kbd>Alt</kbd>+<kbd>←→↑↓</kbd> zone</span>
      <span><kbd>⌘/Ctrl</kbd>+<kbd>O</kbd> open</span>
      <span><kbd>⌘/Ctrl</kbd>+<kbd>D</kbd> diagnostics</span>
      <span><kbd>F11</kbd> fullscreen</span>
      <span><kbd>Space</kbd> play/pause (dumb)</span>
    </div>
  </div>
</div>

<div id="pill"><span class="dot"></span><span class="label" data-c="pill-label">idle</span></div>
<div id="posbar"></div>
<div id="banner">re-syncing…</div>
`;
