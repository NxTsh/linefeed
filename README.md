<p align="center">
  <img src="assets/keycap-512.png" width="140" alt="Linefeed logo — keycap with an LF legend" />
  <br/>
  <img src="assets/wordmark-1024.png" width="340" alt="linefeed wordmark" />
</p>

<p align="center">
  <a href="https://github.com/NxTsh/linefeed/actions/workflows/ci.yml"><img src="https://github.com/NxTsh/linefeed/actions/workflows/ci.yml/badge.svg" alt="ci" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-22D3EE" alt="Apache-2.0" /></a>
  <img src="https://img.shields.io/badge/platforms-Linux%20%C2%B7%20macOS-121217" alt="Linux and macOS" />
</p>

**Linefeed is a teleprompter that scrolls as you speak.** Speech recognition
and script alignment run offline on your machine — no cloud, no accounts, no
uploaded audio.

## Why

Teleprompters that follow your voice usually mean vendor accounts, per-minute
cloud ASR, and your voice shipped to someone else's servers. Linefeed is
local-first by construction: recognition, alignment, and rendering all run on
your machine. Free forever — Apache-2.0, models on disk, nothing to renew.

## Features

- **Voice-following scroll** — the current line highlights and glides to the
  reading anchor; ad-libs hold position, skips catch up, rereads backtrack
- **Two languages in-app**: Portuguese (pt-BR) and English speech models,
  downloaded from the settings panel or on first run
- **Reading zone** — a resizable centered box the text scrolls inside
- **Configurable lookahead** (`[` / `]`) — how many upcoming lines stay visible
- **Reading fonts** — Inter, Atkinson Hyperlegible (low-vision), Source
  Sans 3, Noto Sans, Georgia, system default
- **Mirroring** (horizontal / vertical) for beam-splitter and glass rigs
- **Presentation mode** — `h` hides every control until you bring it back
- **Dumb-scroll fallback** — constant-speed scroll, no microphone needed
- **Session diagnostics** (`Cmd/Ctrl+D`) — tracker events + mic levels as
  JSONL, for triaging a take that misbehaved
- **A real CLI** — `replay` (deterministic), `live` (mic), `devices`,
  `dump` (record replayable timelines); the same alignment core as the GUI

## How it works

1. A streaming ASR engine runs on-device ([sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx))
   and emits word hypotheses with timestamps.
2. An anchored-cursor + banded-DP aligner maps each hypothesis onto the
   script's token stream in real time. Alignment is token-index-only — never
   keyed to font size, visible-word counts, or any rendering metric.
3. The renderer keeps the spoken line at the reading anchor and glides the
   scroll target with the configured lookahead, smoothed by a
   critically-damped spring.

No cloud anywhere in that chain. Audio never leaves the machine; models live
on your disk and are not part of the repo.

### Speech models

| Language | Model | Download | Runs |
|----------|-------|----------|------|
| Português (Brasil) | NVIDIA NeMo FastConformer large (int8) | 99 MB | fully offline |
| English | NVIDIA NeMo Conformer medium (int8) | 158 MB | fully offline |

Both install from the app (settings panel → Voice model) into the platform
data dir (`LINEFEED_MODELS_DIR` overrides). The registry lives in
[`crates/linefeed-asr/src/models.rs`](crates/linefeed-asr/src/models.rs).

## Quickstart

### GUI (Tauri)

```bash
cd crates/linefeed-gui
npm install
npx tauri dev
```

On first run the splash offers the speech model for your selected language;
declining is fine — dumb scroll works without any model. On Linux without
system webkit2gtk dev packages, `source scripts/dev-env.sh` first.

### CLI

```bash
cargo build --release

# list input devices, then track a live reading:
./target/release/linefeed devices
./target/release/linefeed live script.txt --input-device 1 --dump-timeline take.jsonl

# deterministic replay (16 kHz mono WAV, or a recorded timeline):
./target/release/linefeed replay script.txt --wav take.wav
./target/release/linefeed replay script.txt --timeline take.jsonl

# English model:
./target/release/linefeed live script.txt --model en
```

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `h` | Presentation mode — hide all controls (`h` / `Esc` / resting the pointer on the bottom edge brings them back) |
| `f` | Resume auto-follow after a manual scroll |
| `F11` / `Cmd+F` / `Ctrl+F` | Toggle fullscreen |
| `[` / `]` | Lookahead lines down / up |
| `Alt+←/→` | Reading-zone width down / up |
| `Alt+↑/↓` | Reading-zone height down / up |
| `Cmd/Ctrl+D` | Toggle session diagnostics (applies from next session start) |
| `Space` | Play / pause (dumb-scroll mode only) |
| `Cmd/Ctrl+O` | Open a script |

## Development

```
crates/linefeed-core   pure alignment (no audio, no UI, no deps)
crates/linefeed-asr    AsrEngine trait, sherpa engine, model registry, mic
crates/linefeed-cli    binary `linefeed`
crates/linefeed-gui    Tauri v2 app (Rust backend + TS/Vite frontend)
```

```bash
cargo test                                   # core + asr + cli
cargo test -p linefeed-core --features serde # IPC payload contract
cd crates/linefeed-gui && npm test           # frontend pure-module suites
```

Design invariants worth knowing before contributing are in
[`CLAUDE.md`](CLAUDE.md); CI enforces zero-warning builds across every
feature combination. Issues and PRs welcome.

## Status

Alpha, actively developed. The alignment core is validated end-to-end on
recorded pt-BR takes (clean, noisy, ad-lib, and skip-around: final cursor
100% of script tokens). This codebase is a from-scratch rewrite of the
original implementation, carrying its validated algorithm design forward
while fixing the first version's known flaws at the architecture level.

## AI-assisted development

Linefeed is vibe-coded: the implementation is written by
[Claude](https://claude.com/claude-code) (Anthropic) in AI pair-programming
sessions, directed, reviewed, and field-tested by the human maintainer.
Every commit carries a co-authorship trailer. Quality is gated the
old-fashioned way — 110+ unit/contract tests, zero-warning builds across
all feature combinations in CI, and end-to-end validation of the alignment
core against recorded speech takes (100% final-cursor accuracy on clean,
noisy, ad-lib, and skip-around readings).

## Acknowledgements

- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (k2-fsa) — on-device
  speech recognition runtime and model releases
- NVIDIA NeMo — the FastConformer/Conformer acoustic models
- Fonts, all SIL Open Font License 1.1 (licenses ship alongside each family
  in `crates/linefeed-gui/src/assets/fonts/`):
  [SauceCodePro Nerd Font](https://www.nerdfonts.com/) (brand mono),
  [Inter](https://rsms.me/inter/),
  [Atkinson Hyperlegible](https://atkinsonhyperlegiblefont.com/),
  [Source Sans 3](https://github.com/adobe-fonts/source-sans)

## License

Apache-2.0 — see [`LICENSE`](LICENSE).
