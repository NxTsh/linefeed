<p align="center">
  <img src="assets/keycap-512.png" width="140" alt="Linefeed logo — keycap with an LF legend" />
  <br/>
  <img src="assets/wordmark-1024.png" width="340" alt="linefeed wordmark" />
</p>

# Linefeed (nxt)

A clean-room rewrite of [Linefeed](../linefeed), the open-source teleprompter
that scrolls as you speak. Speech recognition and script alignment run offline
on your machine — no cloud, no accounts, no uploaded audio.

This repo reimplements the same product on the same stack (Rust workspace +
Tauri v2 + plain TypeScript/Vite), carrying over the validated algorithm
design and brand/UX decisions while fixing the first implementation's known
design flaws at the architecture level.

## How it works

- A streaming ASR engine runs on-device (sherpa-onnx) and emits word
  hypotheses with timestamps.
- An anchored-cursor + banded-DP aligner maps each hypothesis onto the
  script's token stream in real time; ad-libs hold position, skips catch up,
  rereads backtrack. Alignment is token-index-only — never keyed to font
  size, visible-word counts, or rendering metrics.
- The renderer keeps the spoken line at the reading anchor and glides the
  scroll target with the configured lookahead.

pt-BR-first: normalization, filler handling, and number expansion target
Brazilian Portuguese.

## Layout

```
crates/linefeed-core   pure alignment (no audio, no UI, no deps)
crates/linefeed-asr    AsrEngine trait, sherpa engine, timeline replay, mic
crates/linefeed-cli    binary `linefeed`: replay / live / devices / dump
crates/linefeed-gui    Tauri v2 app (arrives in M4/M5)
```

## Build

```bash
cargo test          # core + asr + cli (GUI excluded from default-members)
cargo build --release
```

ASR models are not in git. Point the tools at a models directory with
`LINEFEED_MODELS_DIR`, or let the GUI download the sherpa pt-BR model on
first run.

## Status

Rewrite in progress. See `CHANGELOG.md` for milestone history.

## License

Apache-2.0 — see [`LICENSE`](LICENSE).
