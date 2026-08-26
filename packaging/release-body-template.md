**Linefeed** — a teleprompter that scrolls as you speak. Recognition and
alignment run fully offline: no cloud, no accounts, no uploaded audio.

## Install

### macOS (Apple Silicon)
Download the `.dmg` (or the `.app` zip), drag **linefeed** to Applications.
The app is not code-signed yet — on first launch macOS will block it:
**right-click the app → Open → Open** (needed once).

### Linux (x86_64)
- Debian/Ubuntu: `sudo apt install ./linefeed_*.deb`
- Any distro: `chmod +x linefeed_*.AppImage && ./linefeed_*.AppImage`

### Windows (x86_64) — experimental
Run the `.msi` installer. Windows builds are not yet human-tested — reports
welcome.

### CLI
Grab the `linefeed-cli-*` archive for your platform; it contains a single
static-friendly `linefeed` binary (`linefeed --help` for usage: replay,
live, devices, dump).

## Speech models (first launch)

Models are not bundled. On first run the app offers to download the model
for your language — Português (Brasil) ~99 MB or English ~158 MB — into your
user data directory; both are also available later from the settings panel
(⚙ → Voice model). Declining is fine: dumb-scroll mode works without any
model. Manual install:

```
mkdir -p "$HOME/.local/share/linefeed/models"   # macOS: ~/Library/Application Support/linefeed/models
curl -L https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-stt_pt_fastconformer_hybrid_large_pc-int8.tar.bz2 | tar -xj -C "$HOME/.local/share/linefeed/models"
```

## Checksums

SHA-256 checksums for every artifact are printed in the release workflow
logs (Actions → release → per-OS "Checksums" step).
