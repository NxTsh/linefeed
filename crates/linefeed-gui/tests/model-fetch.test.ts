import { test } from "node:test";
import assert from "node:assert/strict";
import {
  fetchHoldsSplash,
  fetchStatusLine,
  fmtMB,
  isTerminal,
  nextFetch,
  shouldOfferFetch,
  type FetchUiState,
} from "../src/model-fetch.ts";
import type { ModelFetchEvent, StartupInfoPayload } from "../src/types.ts";

function ev(phase: ModelFetchEvent["phase"], extra: Partial<ModelFetchEvent> = {}): ModelFetchEvent {
  return {
    phase,
    downloaded: 0,
    total: 0,
    pct: 0,
    message: "",
    fatal: phase === "fatal",
    curl: "",
    ...extra,
  };
}

test("happy path: offer → downloading → extracting → ready", () => {
  let s: FetchUiState = "offer";
  s = nextFetch(s, ev("downloading"));
  assert.equal(s, "downloading");
  s = nextFetch(s, ev("extracting"));
  assert.equal(s, "extracting");
  s = nextFetch(s, ev("ready"));
  assert.equal(s, "ready");
  assert.ok(isTerminal(s));
});

test("retry beat: non-fatal failure → retrying → downloading again", () => {
  let s: FetchUiState = "downloading";
  s = nextFetch(s, ev("retrying"));
  assert.equal(s, "retrying");
  s = nextFetch(s, ev("downloading"));
  assert.equal(s, "downloading");
});

test("terminal states swallow late events", () => {
  for (const terminal of ["ready", "cancelled", "declined", "fatal"] as FetchUiState[]) {
    assert.equal(nextFetch(terminal, ev("downloading")), terminal);
    assert.equal(nextFetch(terminal, ev("ready")), terminal);
  }
});

test("fatal carries the curl fallback into the status", () => {
  const s = nextFetch("downloading", ev("fatal", { message: "download failed" }));
  assert.equal(s, "fatal");
  assert.equal(fetchStatusLine(s, ev("fatal", { message: "download failed" })), "download failed");
});

test("splash hold matrix", () => {
  for (const held of ["offer", "downloading", "retrying", "extracting"] as FetchUiState[]) {
    assert.equal(fetchHoldsSplash(held), true, held);
  }
  for (const free of ["absent", "ready", "fatal", "declined", "cancelled"] as FetchUiState[]) {
    assert.equal(fetchHoldsSplash(free), false, free);
  }
});

test("shouldOfferFetch matrix", () => {
  const probe = (models_ok: boolean, fetchable: boolean): StartupInfoPayload => ({
    engines: ["sherpa"],
    engine: "sherpa",
    models_dir: "/m",
    models_ok,
    missing: models_ok ? [] : ["x"],
    fetchable,
    fetch_url: "",
    fetch_bytes: 0,
    last_script: "",
  });
  assert.equal(shouldOfferFetch(probe(false, true)), true);
  assert.equal(shouldOfferFetch(probe(true, true)), false, "model present");
  assert.equal(shouldOfferFetch(probe(false, false)), false, "not fetchable");
  assert.equal(shouldOfferFetch(null), false);
});

test("status lines never emit a bare percent", () => {
  const dl = fetchStatusLine(
    "downloading",
    ev("downloading", { pct: 42, downloaded: 55 * 1024 * 1024, total: 131 * 1024 * 1024 }),
  );
  assert.ok(dl.includes("42%"), dl);
  assert.ok(dl.includes("55 MB"), dl);
  const noTotal = fetchStatusLine("downloading", ev("downloading"));
  assert.ok(!noTotal.includes("%"), `no bare percent: ${noTotal}`);
  assert.equal(fetchStatusLine("declined", null), "skipped — dumb scroll available");
});

test("fmtMB", () => {
  assert.equal(fmtMB(131 * 1024 * 1024), "131 MB");
  assert.equal(fmtMB(0), "0 MB");
});
