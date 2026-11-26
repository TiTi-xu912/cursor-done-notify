#!/bin/bash
# Installs Diandian for Cursor on macOS. In a git checkout with Rust installed it builds
# first; the release zip already carries the programs. Pass --realtime for live tool poses.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
crate="$root/proto/buddy-rs"

cargo="$(command -v cargo || true)"
[ -z "$cargo" ] && [ -x "$HOME/.cargo/bin/cargo" ] && cargo="$HOME/.cargo/bin/cargo"
if [ -f "$crate/Cargo.toml" ] && [ -n "$cargo" ]; then
    (cd "$crate" && "$cargo" build --release --quiet)
    for bin in buddy-host hook-relay; do
        # Replace by rename: overwriting a running binary in place gets it killed on Apple silicon.
        cp "$crate/target/release/$bin" "$root/.$bin.tmp"
        mv -f "$root/.$bin.tmp" "$root/$bin"
    done
fi

for bin in buddy-host hook-relay; do
    if [ ! -x "$root/$bin" ]; then
        echo "找不到 $bin。请先安装 Rust（https://rustup.rs）再运行，或者下载 Releases 里名字带 -macos 的 zip。" >&2
        exit 1
    fi
done

# Downloaded zips are quarantined, and Gatekeeper would block these unsigned programs.
xattr -dr com.apple.quarantine "$root" 2>/dev/null || true

exec "$root/buddy-host" --install "$@"
