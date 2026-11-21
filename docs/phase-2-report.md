# Phase 2 report — CLI foundation (v0 core pipeline)

Status: **complete for the v0 still-image pipeline**. All work verified on
macOS aarch64 (Rust 1.100.0 pinned). Windows x64 and macOS x64 are covered
by CI (`ci.yml` native build jobs) — **not** verified on this machine.

## What shipped

| piece | state |
| --- | --- |
| Workspace (core + 6 codecs + cli) | builds clean; `cargo fmt`/`cargo clippy` silent; lto+strip release binary ≈ 3.3 MB |
| core: model (ADR-004) | StillImage RGBA8 / Decoded::Animated; frames carry composited pixels + timing/offset/blend/disposal |
| core: policy (ADR-005) | EncodeOptions intents; engine owns all data-loss rules |
| core: protocol v1 (plan §7) | Report/input/output/diagnostics/error; stable codes; exit map 0/2/3/4/5/6/7; `to_pretty` cannot fail |
| core: rgba | exact-integer over-composite flattening; all 8 EXIF orientations (unit-tested) |
| core: metadata (ADR-008) | presence probes (JPEG/PNG/RIFF/TIFF/GIF) + `exif_orientation` (JPEG/TIFF containers) |
| codec-webp | direct libwebp FFI; lossless `exact=1` + ARGB import (ADR-002, verified); animated decode (webp-animation); `--frame`; 16383² encoder cap → exit 4 |
| codec-png/jpeg/gif/tiff/bmp | image-crate adapters; APNG→first-frame documented; JPEG/BMP alpha-less by policy; TIFF page count + multi-page refusal; GIF frame compose |
| cli | bare + subcommand forms; `convert`/`inspect`/`formats`/`capabilities`; `--dry-run`; `--json` protocol v1; stdout discipline (ADR-006); atomic writes; overwrite/alias guards; input size/pixel budgets |
| tests | 36 green: 18 core unit, 7 cli unit, 11 e2e integration (assert_cmd, real fixtures built with the vendored encoder) |
| CI | pinned toolchain: fmt/clippy(-D warnings)/test + release builds on macOS arm64/x64, Windows x64 |
| docs | `docs/cli.md` (full reference incl. honest limitations); `docs/dependency-licenses.md` (libwebp COPYING/AUTHORS/PATENTS shipping rule) |

## Verification evidence (this machine)

- `cargo test --workspace`: 36 passed / 0 failed.
- `cargo clippy --workspace --all-targets -D warnings` + `cargo fmt --check`: clean.
- PNG→WebP lossless→PNG round trip: **byte-identical pixels**, including RGB
  under fully-transparent pixels (the exact=1 ADR-002 hazard, covered by a
  regression test, not just the one-off check).
- `otool -L` on the release binary: only `/usr/lib/libSystem.B.dylib` and
  `libiconv` — no libwebp dylib anywhere, vendored static link works.
- Policy refusals verified e2e: JPEG without `--background` → exit 2
  `missing_option`; overwrite without `--overwrite` → exit 5, output
  untouched; input==output alias refused; animated WebP without `--frame`
  → exit 3; `--frame 0/1` decode verified by pixel color.

## Bugs found and fixed during this phase

1. **Encode routed through the input codec** — the first working pipeline
   encoded output bytes with the *input's* codec, so `out.png` could contain
   WebP bytes. Caught by the integration tests (PNG signature check) after
   earlier smoke tests masked it. Fixed: encode is routed to the codec
   registered for the *target* format, with a read-only gate.
2. `libwebp-sys2`'s Rust library name is `libwebp_sys` (not the package
   name) — adapter paths corrected.
3. `webp-animation` needs its `image` cargo feature for `Frame::into_image`.
4. JPEG/GIF encoder API drift vs assumed shapes; GIF output via
   `GifEncoder::encode_frame`.

## Explicitly not done (honest list, carried to next phases)

- Metadata write-through (all formats) — detected + diagnosed only. P1.
- Animated write (animated WebP/GIF output); `--flatten` refused. P1.
- APNG multi-frame read (first frame only, documented in caps).
- Per-page TIFF decode (`--page` reserved, v0 refuses multi-page input).
- Windows/macOS-x64 native verification — deferred to CI runners.
