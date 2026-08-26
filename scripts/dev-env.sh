#!/usr/bin/env bash
# Source this (or use scripts/cargo.sh) to build mic/GUI features on a Linux
# box WITHOUT sudo or system dev packages (alsa, webkit2gtk, gtk).
#
# It points pkg-config at a local .deb-extracted sysroot. Resolution order:
#   1. $LINEFEED_SYSROOT
#   2. ./.webkit-sysroot            (this repo, once fetched)
#   3. ../linefeed/app/.webkit-sysroot  (the sibling first-implementation repo)
#
# CI runners install real dev packages via apt instead and never need this.
#
# macOS needs NONE of this (CoreAudio + WKWebView are system frameworks):
# sourcing here is a clean no-op.

if [ "$(uname -s)" = "Darwin" ]; then
    echo "dev-env: macOS — no sysroot needed, nothing to do" >&2
    return 0 2>/dev/null || exit 0
fi

_lf_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

_lf_sysroot=""
for cand in "${LINEFEED_SYSROOT:-}" "$_lf_root/.webkit-sysroot" "$_lf_root/../linefeed/app/.webkit-sysroot"; do
    if [ -n "$cand" ] && [ -f "$cand/usr/lib/x86_64-linux-gnu/pkgconfig/alsa.pc" ]; then
        _lf_sysroot="$cand"
        break
    fi
done

if [ -z "$_lf_sysroot" ]; then
    echo "dev-env: no sysroot found (looked for usr/lib/x86_64-linux-gnu/pkgconfig/alsa.pc)" >&2
    echo "dev-env: set LINEFEED_SYSROOT or fetch one (see scripts/ in the GUI milestone)" >&2
    return 1 2>/dev/null || exit 1
fi

export PKG_CONFIG_PATH="$_lf_sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$_lf_sysroot/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="$_lf_sysroot"
export LIBRARY_PATH="$_lf_sysroot/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}"
export LD_LIBRARY_PATH="$_lf_sysroot/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
echo "dev-env: sysroot = $_lf_sysroot" >&2
