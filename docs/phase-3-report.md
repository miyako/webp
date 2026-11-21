# Phase 3 report — static conversion matrix

Status: **complete — gate passed** ("supported static conversion matrix
passes", plan §"Phase 3"). All work verified on macOS aarch64 (Rust 1.100.0
pinned). Windows x64 and macOS x64 *native* execution remain CI-covered only
(see below for the new local Universal-binary evidence, which *is* local but
whose x86_64 slice is not executed here).

## What shipped this phase

| piece | state |
| --- | --- |
| Committed fixture corpus (tests/fixtures/) | static canonicals (48×32 opaque gradient) in png/jpeg/gif/webp/tiff/bmp; RGBA-over-transparent png+webp pair; two-frame animated webp reproducible via `cargo run -p webp-convert-codec-webp --example write_anim_fixture`; malformed set (truncated: png/gif/webp; corrupt: gif palette, png signature-only, webp riff-short, not-an-image) |
| Conversion-matrix suite (crates/cli/tests/matrix.rs) | 7 tests, all green; the full 6×6 = 36 pair matrix converts with dimensions preserved, plus targeted tests for round-trip exactness, alpha preservation, animation, malformed exit codes, empty stdin, jpeg→alpha targets |
| `WEBP_CONVERT_BIN` override | runs the suite against a prebuilt binary (release or the local Universal build); repo-root-relative paths resolve even though cargo runs tests from crates/cli |
| licenses/libwebp/ | vendored libwebp 1.5.0 redistribution files (COPYING, AUTHORS, PATENTS) + libwebp-sys2 binding license, per docs/dependency-licenses.md; refreshable via `scripts/refresh-licenses.sh` |
| scripts/release-manifest.sh (§11.5) | per-release JSON manifest: CLI version, commit (dirty flag), target, toolchain pin, UTC date, binary SHA-256 + size, the binary's own capabilities report, dependency inventory from Cargo.lock, license inventory from cargo metadata (jq), provenance; output in gitignored manifests/ |
| scripts/build-macos-universal.sh (§11.2) | builds arm64 + x86_64 release, lipo-merges, smoke-tests |

## Verification evidence (this machine)

- Matrix suite: 7/7 against the debug-built binary (163 s — 36 subprocess
  conversions in debug) **and** 7/7 in 3.2 s via
  `WEBP_CONVERT_BIN=target/release/webp-convert`.
- Full workspace: 43 tests green (18 core unit, 7 cli unit, 11 e2e,
  7 matrix); `cargo fmt --check` and
  `cargo clippy --workspace --all-targets -D warnings` clean.
- **macOS Universal binary (§11.2)**: `scripts/build-macos-universal.sh`
  built both slices with the vendored static libwebp (x86_64 cross-compile
  of the C code succeeds), `lipo -info` reports `x86_64 arm64`, and the
  arm64 slice passes `formats` + a real png→webp conversion. The x86_64
  slice is **not executed locally** (Rosetta 2 not installed) — native x64
  execution is verified by the CI macos-13 job.
- Release manifest: generated end-to-end on this machine; 104 workspace
  deps inventoried (96 external), 96 license entries matching 1:1, libwebp
  provenance note present. `jq` required (present on macOS/GitHub runners);
  script exits with a clear error when missing.
- License refresh script: re-copied from the cargo registry and verified
  against `configure.ac` (`AC_INIT([libwebp], [1.5.0]`); idempotent.

## Bugs found and fixed during this phase

1. Matrix test named outputs `.out` (unknown format → exit 2 before any
   conversion logic ran). Outputs now carry their real extension.
2. `WEBP_CONVERT_BIN` paths relative to the repo root failed under cargo's
   test cwd (crates/cli); `resolve_bin` tries the manifest dir's parents.
3. `Cargo.toml` pins libwebp-sys2 loosely (`"0.2"`); the cookie-cutter
   version probe kept matching a *transitive* 0.1.11 in the lock. The
   manifest/release scripts now resolve the highest locked version from
   `Cargo.lock` and the registry dir from the resolved version.

## Explicitly not done (honest list, carried to next phases)

- Metadata write-through (all formats) — detected + diagnosed only. P1,
  Phase 5 scope.
- Animated write; `--flatten`; per-page TIFF (`--page` reserved).
- x86_64 slice execution (needs Rosetta 2 or a native x64 machine).
- Windows x64 native run — CI only.
- Signing/notarization of the Universal binary — external infrastructure,
  Phase 6/release time.
