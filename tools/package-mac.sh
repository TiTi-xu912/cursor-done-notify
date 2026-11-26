#!/bin/bash
# Builds a universal (Apple silicon + Intel) macOS package: dist/cursor-done-notify-v<version>-macos.zip.
# Pass the release tag (v1.2.3) to check it against the version in Cargo.toml and Host.cs.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
crate="$root/proto/buddy-rs"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$crate/Cargo.toml" | head -1)"
host_version="$(sed -n 's/.*public const string Version = "\(.*\)";.*/\1/p' "$root/Host.cs" | head -1)"
if [ "$version" != "$host_version" ]; then
    echo "Cargo.toml version $version does not match AppInfo.Version $host_version in Host.cs" >&2
    exit 1
fi
if [ $# -gt 0 ] && [ "${1#[vV]}" != "$version" ]; then
    echo "Tag $1 does not match version $version" >&2
    exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin > /dev/null
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd "$crate" && RUSTFLAGS="-D warnings" cargo build --release --target "$target")
done

name="cursor-done-notify-v$version-macos"
stage="$root/dist/$name"
rm -rf "$stage" "$stage.zip"
mkdir -p "$stage/sprites/poses"
for bin in buddy-host hook-relay; do
    lipo -create -output "$stage/$bin" \
        "$crate/target/aarch64-apple-darwin/release/$bin" \
        "$crate/target/x86_64-apple-darwin/release/$bin"
    # Apple silicon refuses to run unsigned code; an ad-hoc signature is enough.
    codesign --force --sign - "$stage/$bin"
done
cp "$root/install.sh" "$root/uninstall.sh" "$root/README.md" "$root/LICENSE" "$stage/"
cp "$root"/sprites/poses/*.png "$stage/sprites/poses/"

(cd "$root/dist" && ditto -c -k --keepParent "$name" "$name.zip")
echo "$root/dist/$name.zip"
