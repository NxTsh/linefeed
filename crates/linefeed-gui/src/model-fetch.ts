// First-run model download state machine (pure). The splash owns the UI;
// this module owns the states:
//
//   absent → offer → downloading ⇄ retrying → extracting → ready
//                 ↘ declined                  ↘ fatal (curl fallback)
//   downloading/retrying/extracting → cancelled (Esc)
//
// Declining is first-class — dumb scroll always works.

import type { ModelFetchEvent, StartupInfoPayload } from "./types.ts";

export type FetchUiState =
  | "absent"
  | "offer"
  | "downloading"
  | "retrying"
  | "extracting"
  | "ready"
  | "fatal"
  | "declined"
  | "cancelled";

const TERMINAL: ReadonlySet<FetchUiState> = new Set(["ready", "fatal", "declined", "cancelled"]);

export function isTerminal(s: FetchUiState): boolean {
  return TERMINAL.has(s);
}

/** Should the splash offer the download? Only when the engine's model is
 * missing AND this build can fetch it. */
export function shouldOfferFetch(probe: StartupInfoPayload | null): boolean {
  return !!probe && !probe.models_ok && probe.fetchable;
}

/** Fold a backend event into the UI state. Terminal states swallow late
 * events (a straggling progress event after cancel must not resurrect the
 * download UI). */
export function nextFetch(cur: FetchUiState, ev: ModelFetchEvent): FetchUiState {
  if (isTerminal(cur)) return cur;
  switch (ev.phase) {
    case "starting":
    case "downloading":
      return "downloading";
    case "retrying":
      return "retrying";
    case "extracting":
      return "extracting";
    case "ready":
      return "ready";
    case "cancelled":
      return "cancelled";
    case "fatal":
      return "fatal";
  }
}

/** While true, the splash stays up. */
export function fetchHoldsSplash(s: FetchUiState): boolean {
  return s === "offer" || s === "downloading" || s === "retrying" || s === "extracting";
}

export function fmtMB(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} MB`;
}

/** One-line status for the splash. Never emits a bare "%" with no number. */
export function fetchStatusLine(s: FetchUiState, ev: ModelFetchEvent | null): string {
  switch (s) {
    case "downloading":
      return ev && ev.total > 0
        ? `downloading… ${ev.pct}% (${fmtMB(ev.downloaded)} / ${fmtMB(ev.total)})`
        : "downloading…";
    case "retrying":
      return ev?.message ?? "retrying…";
    case "extracting":
      return "extracting…";
    case "ready":
      return "model installed";
    case "fatal":
      return ev?.message ?? "download failed";
    case "cancelled":
      return "download cancelled";
    case "declined":
      return "skipped — dumb scroll available";
    default:
      return "";
  }
}
