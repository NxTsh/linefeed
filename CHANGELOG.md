# Changelog

All notable changes to this project will be documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: SemVer.

## [Unreleased]

Clean-room rewrite of the original Linefeed repo, carrying over the
validated algorithm design and brand/UX decisions while fixing the first
implementation's surveyed design flaws at the architecture level.

### Added
- M0: repo scaffold — workspace at the root (`linefeed-core`, `linefeed-asr`,
  `linefeed-cli`, `linefeed-gui`), brand assets, project docs.
- M1: alignment core — pt-BR normalization with accent-folded fillers,
  digit→words expansion, scratch-buffer windowed DP (no per-cell allocation),
  4-state tracker (TRACKING/HOLDING/LOST/BACKTRACK) with a bounded retained
  stream. 27 tests incl. a red-team suite and a serde roundtrip that runs in
  every build.
- M2: ASR — `AsrEngine` trait, sherpa-onnx trailing-window engine (1 s hop,
  12 s window, clamped time anchors), timeline JSONL replay/writer, mic
  pipeline (config negotiation, max-RMS channel selection with hysteresis,
  rubato 16 kHz, bounded drop-oldest capture buffer with coalesced reads).
- M3: CLI — subcommands `replay` / `live` / `devices` / `dump`;
  `--dump-timeline` works everywhere incl. live; Ctrl-C exits 0 with the
  final summary; tested zero-dep arg parser.
- M4: GUI backend (Tauri v2) — async start/stop, token-span `ScriptPayload`
  (Rust owns tokenization), sanitized persisted config, platform models dir,
  splash-managed first-run model download (staging, tar allow-list, verify,
  retry, cancel, curl fallback), opt-in JSONL diagnostics.
- M5: GUI frontend (TS/Vite, no framework) — visual-line renderer with
  adaptive ⅓ anchor + lookahead + intra-row glide, closed-form spring,
  manual-scroll follow suspend, presentation mode with a single visibility
  funnel, splash with a 3 s brand dwell, model-fetch UI. 67 node tests incl.
  TS↔Rust contract pins.
- M6: CI — platform matrix, zero-warning feature-combo guard, GUI job,
  deterministic timeline-replay E2E (no models needed).

### Fixed (relative to the first implementation, by design)
- Accented filler words (`não`, `já`, `há`, `né`, `lá`…) now actually
  classify as fillers (the list is folded at build time).
- sherpa word timestamps can no longer shift silently when the decode window
  predates the retained audio.
- Live mic sessions exit cleanly (code 0 + summary) and can record
  `--dump-timeline`; decode-slower-than-realtime now drops oldest audio
  (bounded latency) instead of growing an unbounded queue.
- GUI `stop` no longer stalls the UI; Space no longer silently switches a
  voice session to dumb scroll; the anchor guide tracks the computed anchor;
  zone/lead keyboard steps no longer race async config reads.
- Tracker memory is bounded; alignment allocates nothing per DP cell.

E2E validated against the first repo's recorded pt-BR takes + sherpa model:
f1-clean, f1-noisy, f2-adlib, f3-skiparound all reach 262/262 (100.0%).
