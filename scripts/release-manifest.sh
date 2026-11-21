#!/bin/sh
# Release manifest generator (plan section 11.5).
#
# Emits one JSON manifest per binary containing: CLI version, commit,
# target, codec/backend versions (from the CLI's own capability report),
# build date, SHA-256, dependency inventory (Cargo.lock), and license
# inventory (cargo metadata; requires jq).
#
# Usage: scripts/release-manifest.sh [path-to-binary] [output-dir]
#   binary defaults to target/release/webp-convert(.exe on Windows)
#   output defaults to manifests/ under the repo root (created)
#
# Notes:
#   - The binary must already be built for the target you want recorded.
#   - Run on a machine matching the binary's target (records `rustc -vV`).
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
BIN="${1:-}"
OUT_DIR="${2:-$ROOT/manifests}"

if [ -z "$BIN" ]; then
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) BIN="$ROOT/target/release/webp-convert.exe" ;;
        *) BIN="$ROOT/target/release/webp-convert" ;;
    esac
fi

if [ ! -x "$BIN" ] && [ ! -f "$BIN" ]; then
    echo "error: binary not found: $BIN (build it first)" >&2
    exit 1
fi

command -v jq >/dev/null 2>&1 || {
    echo "error: jq is required (license inventory); install jq and re-run" >&2
    exit 1
}

cd "$ROOT"

# --- identity -------------------------------------------------------------

CLI_VERSION="$("$BIN" --version | awk '{print $2}')"
COMMIT="$(git rev-parse HEAD)"
DIRTY=""
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    DIRTY="-dirty"
fi
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
RUSTC_VERSION="$(rustc -vV | sed -n 's/^rustc //p')"
TOOLCHAIN_PIN="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml | head -1)"
BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
BUILD_DATE_SHORT="$(date -u +%Y%m%d)"

# --- checksum -------------------------------------------------------------

if command -v shasum >/dev/null 2>&1; then
    SHA256_BIN="$(shasum -a 256 "$BIN" | awk '{print $1}')"
elif command -v sha256sum >/dev/null 2>&1; then
    SHA256_BIN="$(sha256sum "$BIN" | awk '{print $1}')"
else
    echo "error: neither shasum nor sha256sum available" >&2
    exit 1
fi
BIN_SIZE="$(wc -c < "$BIN" | tr -d '[:space:]')"

# --- capability manifest (from the binary itself, its own report) ---------

CAPABILITIES="$("$BIN" capabilities --json)"
CAPS_SHA256=""
if command -v shasum >/dev/null 2>&1; then
    CAPS_SHA256="$(printf '%s' "$CAPABILITIES" | shasum -a 256 | awk '{print $1}')"
elif command -v sha256sum >/dev/null 2>&1; then
    CAPS_SHA256="$(printf '%s' "$CAPABILITIES" | sha256sum | awk '{print $1}')"
fi

# --- dependency inventory (Cargo.lock, no external tools) ----------------

DEPS_JSON="$(awk 'BEGIN { printf "[" }
    $0 == "[[package]]" {
        if (name != "" && version != "") {
            printf "%s{\"name\":\"%s\",\"version\":\"%s\",\"external\":%s}", (n ? "," : ""), name, version, (sourced ? "true" : "false")
            n = 1
        }
        inpkg = 1; name = ""; version = ""; sourced = 0; next
    }
    inpkg && $1 == "name"        { name = $3; gsub(/"/, "", name); next }
    inpkg && $1 == "version"     { version = $3; gsub(/"/, "", version); next }
    inpkg && $1 == "source"      { sourced = 1 }
    END {
        if (name != "" && version != "") {
            printf "%s{\"name\":\"%s\",\"version\":\"%s\",\"external\":%s}", (n ? "," : ""), name, version, (sourced ? "true" : "false")
        }
        printf "]"
    }
' Cargo.lock)"

# --- license inventory (cargo metadata; third-party crates only) ---------

LICENSES_JSON="$(cargo metadata --format-version 1 --locked 2>/dev/null | jq -c '
    [ .packages[]
      | select(.source != null)
      | { name: .name
        , version: .version
        , license: (.license // (.license_file // "UNKNOWN"))
        }
    ] | sort_by(.name, .version)
')"

# --- libwebp vendored version (license-shipped native code) ---------------

LIBWEBP_FILE="$ROOT/licenses/libwebp/COPYING"
if [ -f "$LIBWEBP_FILE" ]; then
    LIBWEBP_NOTE="licensing files shipped in licenses/libwebp/ (see licenses/README.md)"
else
    LIBWEBP_NOTE="WARNING: licenses/libwebp/ missing — run scripts/refresh-licenses.sh"
fi

# --- emit ------------------------------------------------------------------

MANIFEST_NAME="webp-convert-${CLI_VERSION}-${TARGET}${DIRTY}-${BUILD_DATE_SHORT}"
OUT_FILE="${OUT_DIR}/${MANIFEST_NAME}.manifest.json"
mkdir -p "$OUT_DIR"

jq -n \
    --arg schema "webp-convert-release-manifest/1" \
    --arg cli_version "$CLI_VERSION" \
    --arg commit "$COMMIT" \
    --arg commit_status "$([ -n "$DIRTY" ] && echo dirty || echo clean)" \
    --arg target "$TARGET" \
    --arg rustc "$RUSTC_VERSION" \
    --arg toolchain_pin "$TOOLCHAIN_PIN" \
    --arg build_date "$BUILD_DATE" \
    --arg binary "$BIN" \
    --arg binary_sha256 "$SHA256_BIN" \
    --arg binary_size_bytes "$BIN_SIZE" \
    --arg capabilities_sha256 "$CAPS_SHA256" \
    --argjson capabilities "$CAPABILITIES" \
    --argjson dependencies "$DEPS_JSON" \
    --argjson licenses "$LICENSES_JSON" \
    --arg libwebp "$LIBWEBP_NOTE" \
    '{
        manifest_schema: $schema,
        cli_version: $cli_version,
        commit: $commit,
        commit_status: $commit_status,
        target: $target,
        toolchain: { rustc: $rustc, pinned_channel: $toolchain_pin },
        build_date_utc: $build_date,
        binary: { path: $binary, sha256: $binary_sha256, size_bytes: ($binary_size_bytes | tonumber) },
        capabilities_report: $capabilities,
        capabilities_report_sha256: $capabilities_sha256,
        dependency_inventory: $dependencies,
        license_inventory: $licenses,
        provenance: {
            repository: "https://github.com/miyako/webp",
            toolchain_pinned_by: "rust-toolchain.toml",
            lockfile: "Cargo.lock committed; cargo metadata run with --locked",
            native_vendored_dependencies: [ $libwebp ]
        }
    }' > "$OUT_FILE"

echo "manifest written: $OUT_FILE"

# --- accompanying checksum file --------------------------------------------

if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$OUT_FILE" > "${OUT_FILE}.sha256"
elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$OUT_FILE" > "${OUT_FILE}.sha256"
fi
echo "checksum written: ${OUT_FILE}.sha256"
