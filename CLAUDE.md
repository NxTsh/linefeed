# CLAUDE.md — agent context for linefeed-nxt

Clean-room rewrite of the Linefeed teleprompter (see sibling repo `../linefeed`).
Same stack: Rust workspace + Tauri v2 + plain TS/Vite. pt-BR-first. Apache-2.0.

## Layout

Workspace at the repo root (no `app/` nesting).

- `crates/linefeed-core` — pure alignment: normalization, banded DP matching,
  4-state cursor tracker. NO audio or UI dependencies — keep it that way.
- `crates/linefeed-asr` — `AsrEngine` trait; sherpa-onnx engine (offline CTC +
  trailing-window re-decode); timeline JSONL replay engine; mic pipeline
  (cpal + max-RMS channel mux + rubato 16 kHz, bounded coalescing channel).
- `crates/linefeed-cli` — binary `linefeed`, subcommands: `replay`, `live`,
  `devices`, `dump`.
- `crates/linefeed-gui` — Tauri v2 app (M4/M5; excluded from default-members).

## Commands

- Tests: `cargo test` from the repo root (GUI excluded by default-members).
- Core with serde: `cargo test -p linefeed-core --features serde` (CI runs both).
- Lint gate: `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`.

## Design invariants (carried over + hardened)

- ALL alignment logic lives in `linefeed-core`, token-index-based. GUI is a
  pure renderer. Spring smoothing is frontend-only (TS).
- Filler set is accent-folded and deduped at build time — filler literals are
  compared post-normalization, always.
- Matcher uses reusable flat scratch buffers; no per-cell allocation. LOST
  scanning is bounded.
- Tracker retained stream is bounded (rebased absolute indices), cursor never
  decreases except on Backtrack events.
- Mic → engine channel is bounded with coalescing (back-pressure by design).
- Library crates never panic on bad input. Tests are never weakened to pass.
- sherpa-onnx >= 1.13.6 (1.13.5 osx-arm64 static bundle aborts at load).
- Feature names are unified across crates: `sherpa`, `mic` (vosk deferred).
- Models dir: `LINEFEED_MODELS_DIR` env, else the platform data dir. No
  compile-time dev paths. The model registry (`linefeed-asr/src/models.rs`)
  is the single source of truth for downloadable models (pt-br, en) — when
  adding one, LIST THE ACTUAL TARBALL first (entries may be `./`-prefixed;
  `model.int8.onnx` + `tokens.txt` must sit under one top-level dir).
- Dark theme only: bg `#0A0A0A`, text `#E8E3D9`, accent cyan `#22D3EE`.
  Status green/amber/red are semantic — never reassign them.

## Conventions

- Commit identity: `Moanga Coder <39803009+caiodelgadonew@users.noreply.github.com>`;
  agents never push — the owner pushes.
- rustfmt clean; zero-warning builds.
