#!/bin/sh
# Refresh vendored libwebp license files from the cargo registry.
# Run after bumping libwebp-sys2. Verifies the vendored libwebp version
# and re-copies COPYING/AUTHORS/PATENTS plus the binding-crate license.
set -eu

CRATE_NAME="libwebp-sys2"

# Exact version comes from Cargo.lock: Cargo.toml pins loosely ("0.2"),
# the registry directory needs the resolved version ("0.2.0"). Several
# versions may be in the lock (transitive deps); take the highest.
# awk for portability (no tomllib on older python3).
CRATE_VERSION="$(awk -v name="$CRATE_NAME" '
    $0 == "[[package]]" { inpkg = 1; name_ok = 0; next }
    inpkg && $1 == "name" && $3 == "\"" name "\"" { name_ok = 1; next }
    inpkg && name_ok && $1 == "version" {
        v = $3; gsub(/"/, "", v);
        if (v > best) best = v; name_ok = 0; next
    }
    /^\[\[/ { inpkg = 0 }
    END { print best }
' Cargo.lock)"

if [ -z "${CRATE_VERSION}" ]; then
    echo "error: cannot determine libwebp-sys2 version from Cargo.toml/Cargo.lock" >&2
    exit 1
fi

if [ ! -d "${CARGO_HOME:-$HOME/.cargo}/registry" ]; then
    echo "error: cargo registry not found under ${CARGO_HOME:-$HOME/.cargo}" >&2
    exit 1
fi

SRC_DIR="$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 2 -type d -name "${CRATE_NAME}-${CRATE_VERSION}" | head -1)"
if [ -z "${SRC_DIR}" ]; then
    echo "error: vendored sources for ${CRATE_NAME}-${CRATE_VERSION} not in local registry; run 'cargo fetch' first" >&2
    exit 1
fi

LIBWEBP_VERSION="$(sed -n 's/^AC_INIT(\[libwebp\], \[\([0-9.]*\)\].*/\1/p' "${SRC_DIR}/c_src/configure.ac")"
if [ -z "${LIBWEBP_VERSION}" ]; then
    echo "error: cannot read libwebp version from ${SRC_DIR}/c_src/configure.ac" >&2
    exit 1
fi
echo "vendored libwebp version: ${LIBWEBP_VERSION} (via ${CRATE_NAME} ${CRATE_VERSION})"

OUT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)/licenses/libwebp"
mkdir -p "${OUT_DIR}"
cp "${SRC_DIR}/c_src/COPYING" "${OUT_DIR}/COPYING"
cp "${SRC_DIR}/c_src/AUTHORS" "${OUT_DIR}/AUTHORS"
cp "${SRC_DIR}/c_src/PATENTS" "${OUT_DIR}/PATENTS"
cp "${SRC_DIR}/LICENSE" "${OUT_DIR}/LICENSE-libwebp-sys2.txt"

echo "updated licenses/libwebp/ — commit the diff if it changed."
echo "note: also update the version in licenses/README.md (${LIBWEBP_VERSION})."
