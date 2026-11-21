#!/bin/sh
# Build a macOS Universal (arm64 + x86_64) webp-convert release binary
# (plan section 11.2).
#
# - Installs missing std targets via rustup (network access required once).
# - Builds each target in release mode.
# - Merges with lipo and smoke-tests both slices (x86_64 via Rosetta 2 if
#   available).
#
# Usage: scripts/build-macos-universal.sh
# Output: target/universal/webp-convert (+ .manifest note printed)
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

TARGET_ARM="aarch64-apple-darwin"
TARGET_X64="x86_64-apple-darwin"
OUT_DIR="$ROOT/target/universal"
BIN_ARM="$ROOT/target/$TARGET_ARM/release/webp-convert"
BIN_X64="$ROOT/target/$TARGET_X64/release/webp-convert"
BIN_UNI="$OUT_DIR/webp-convert"

TOOLCHAIN=""
if grep -q "^channel" rust-toolchain.toml 2>/dev/null; then
    TOOLCHAIN="+$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml | head -1)"
    echo "using pinned toolchain: $TOOLCHAIN"
fi

installed_targets() {
    rustup $TOOLCHAIN target list --installed 2>/dev/null || true
}

for T in "$TARGET_ARM" "$TARGET_X64"; do
    if ! installed_targets | grep -qx "$T"; then
        echo "installing missing std target: $T"
        rustup $TOOLCHAIN target add "$T"
    fi
done

echo "building $TARGET_ARM ..."
cargo $TOOLCHAIN build --release --target "$TARGET_ARM"
echo "building $TARGET_X64 ..."
cargo $TOOLCHAIN build --release --target "$TARGET_X64"

mkdir -p "$OUT_DIR"
echo "merging with lipo ..."
lipo -create "$BIN_ARM" "$BIN_X64" -output "$BIN_UNI"
lipo -info "$BIN_UNI"
file "$BIN_UNI"

echo "--- smoke tests ---"
"$BIN_UNI" formats >/dev/null && echo "arm64 slice: OK"
if arch -x86_64 /usr/bin/true >/dev/null 2>&1; then
    arch -x86_64 "$BIN_UNI" formats >/dev/null && echo "x86_64 slice (Rosetta): OK"
else
    echo "x86_64 slice: built but not run (Rosetta 2 not installed)"
fi

echo "universal binary: $BIN_UNI"
