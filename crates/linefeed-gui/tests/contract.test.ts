// Source-text contract tests: pin the TS↔Rust IPC agreement and the
// shipped chrome without a DOM. These intentionally read source files —
// they break when the contract breaks, which is the point.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { CONTROLS_TEMPLATE } from "../src/controls-template.ts";
import { READING_FONTS } from "../src/pipeline.ts";
import { EV } from "../src/api.ts";

const here = new URL(".", import.meta.url).pathname;
const src = (p: string) => readFileSync(join(here, "..", "src", p), "utf8");
const rust = (p: string) => readFileSync(join(here, "..", "src-tauri", "src", p), "utf8");

test("withGlobalTauri is on (api.ts detects Tauri via window.__TAURI__)", () => {
  const conf = JSON.parse(
    readFileSync(join(here, "..", "src-tauri", "tauri.conf.json"), "utf8"),
  );
  assert.equal(conf.app.withGlobalTauri, true, "without it every invoke silently no-ops");
  assert.ok(src("api.ts").includes("__TAURI__"), "api.ts relies on the injected global");
});

test("event names match the Rust constants", () => {
  const lib = rust("lib.rs");
  for (const name of Object.values(EV)) {
    assert.ok(lib.includes(`"${name}"`), `Rust must declare ${name}`);
  }
});

test("reading font ids match Rust READING_FONT_IDS", () => {
  const cfg = rust("config.rs");
  for (const f of READING_FONTS) {
    assert.ok(cfg.includes(`"${f.id}"`), `Rust must allow-list ${f.id}`);
  }
});

test("zone and wpm clamps match Rust sanitize", () => {
  const cfg = rust("config.rs");
  assert.ok(cfg.includes("READING_WIDTH_RANGE: (u32, u32) = (40, 100)"));
  assert.ok(cfg.includes("READING_HEIGHT_RANGE: (u32, u32) = (30, 100)"));
  assert.ok(cfg.includes("WPM_RANGE: (u32, u32) = (40, 400)"));
  assert.ok(cfg.includes("FONT_PX_RANGE: (u32, u32) = (24, 200)"));
});

test("every invoke command exists in the Rust handler list", () => {
  const apiSrc = src("api.ts");
  const lib = rust("lib.rs");
  const cmds = [...apiSrc.matchAll(/invoke<[^>]*>\("([a-z_]+)"/g)].map((m) => m[1]!);
  assert.ok(cmds.length >= 20, `found ${cmds.length} commands`);
  for (const c of cmds) {
    assert.ok(new RegExp(`\\b${c}\\b`).test(lib), `Rust must handle ${c}`);
  }
});

test("chrome visibility has exactly ONE write site (the funnel)", () => {
  const controls = src("controls.ts");
  const writes = controls.match(/classList\.toggle\("chrome-off"/g) ?? [];
  assert.equal(writes.length, 1, "only applyVisibility touches chrome-off");
  for (const file of ["main.ts", "prompter.ts"]) {
    assert.ok(!src(file).includes('"chrome-off"'), `${file} must not touch chrome-off`);
  }
});

test("shipped chrome: no Debug button, unique data-c ids", () => {
  assert.ok(!/debug/i.test(CONTROLS_TEMPLATE), "debug is a chord, not a button");
  const ids = [...CONTROLS_TEMPLATE.matchAll(/data-c="([a-z-]+)"/g)].map((m) => m[1]!);
  assert.ok(ids.includes("open") && ids.includes("present") && ids.includes("mode"));
  assert.equal(new Set(ids).size, ids.length, "no duplicate action ids");
});

test("settings panel: gear on the bar, setup controls in the panel", () => {
  const bar = CONTROLS_TEMPLATE.slice(0, CONTROLS_TEMPLATE.indexOf("settings-panel"));
  const panel = CONTROLS_TEMPLATE.slice(CONTROLS_TEMPLATE.indexOf("settings-panel"));
  assert.ok(bar.includes('data-c="settings"'), "gear lives on the bar");
  // Session-time controls stay on the bar…
  for (const id of ["open", "start", "device", "mode", "present"]) {
    assert.ok(bar.includes(`data-c="${id}"`), `${id} belongs on the bar`);
  }
  // …setup-once controls live in the panel.
  for (const id of ["reading-font", "zone-w-down", "mirror-h", "lead-up", "model", "model-list"]) {
    assert.ok(panel.includes(`data-c="${id}"`), `${id} belongs in the panel`);
  }
  assert.ok(panel.includes("<h3>"), "panel has labeled sections");
});

test("presentation hide covers the settings panel too", () => {
  const css = src("styles.css");
  const chromeOff = css.slice(css.indexOf("body.chrome-off"));
  assert.ok(chromeOff.includes("#settings-panel"), "panel must vanish with chrome");
});

test("model picker ids match the Rust registry", () => {
  const registry = readFileSync(
    join(here, "..", "..", "linefeed-asr", "src", "models.rs"),
    "utf8",
  );
  for (const id of ["pt-br", "en"]) {
    assert.ok(registry.includes(`id: "${id}"`), `registry must define ${id}`);
  }
  assert.ok(registry.includes(`DEFAULT_MODEL_ID: &str = "pt-br"`));
});

test("index.html: splash brand and fetch box ship with first paint", () => {
  const html = readFileSync(join(here, "..", "index.html"), "utf8");
  assert.ok(html.includes("#0a0a0a"), "brand background inline");
  assert.ok(html.includes("#22d3ee"), "brand accent inline");
  assert.ok(html.includes('id="fetch-box"'));
  assert.ok(html.includes('id="error-box"'));
  assert.ok(html.includes('id="zone"'), "zone wrapper is load-bearing");
  assert.ok(html.includes('id="anchor"'));
});

test("anchor guide is computed, not hard-coded (first-repo regression)", () => {
  const css = src("styles.css");
  const anchorBlock = css.slice(css.indexOf("#anchor"), css.indexOf("#row-bar"));
  assert.ok(!/^\s*top:/m.test(anchorBlock), "no static top property on #anchor in CSS");
  assert.ok(src("prompter.ts").includes("anchorEl.style.top = `${this.anchor}px`"));
});

test("space key never force-switches scroll mode (first-repo regression)", () => {
  const main = src("main.ts");
  const spaceBlock = main.slice(main.indexOf('e.key === " "'), main.indexOf('e.key === "["'));
  assert.ok(spaceBlock.includes('scroll_mode === "dumb"'), "space is dumb-mode gated");
  assert.ok(!spaceBlock.includes("setScrollMode"), "space never changes the mode");
});
