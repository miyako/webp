# Phase 6 report — hardening and CLI release gate

Status: **complete for the v1.0.0 gate** (plan §"Phase 6"). Verified on
macOS aarch64 (Rust 1.100.0 stable, release profile) unless marked otherwise.

## What shipped this phase

| piece | state |
| --- | --- |
| Metadata write-through (plan §5.6 / ADR-008, chunk D) | v1: ICC/EXIF/XMP extracted from JPEG/PNG/WebP/TIFF containers and re-embedded into WebP output (VP8X/ICCP/EXIF/XMP RIFF surgery, `metadata_carried`); `--metadata icc` single-category mode; `--metadata none` per-category strip infos; EXIF orientation rewritten to 1 when pixels are rotated (`--orient auto`) so viewers don't double-rotate; non-WebP targets still drop with loud `metadata_dropped` warnings |
| Fuzzing | two libfuzzer targets under `fuzz/` (cargo-fuzz 0.13.2, nightly, ASan): `sniff_extract_decode` (sniff → inspect → decode → WebP encode round-trip over all six codecs + metadata extraction/patching) and `webp_container` (RIFF mux surgery on arbitrary bytes, replacement path, orientation patcher). Seed corpora from 21 test fixtures. Declared-dimension gates keep absurd headers from OOMing the harness (the CLI has its own guards) |
| Resource-limit hardening | three real defects found by fuzzing and fixed (below) |
| Security audit | `cargo audit` (0.23.0, advisory DB 1555 entries): **0 vulnerabilities, 0 warnings** for the main Cargo.lock (104 dependencies) and for `fuzz/Cargo.lock` |
| Performance | timing matrix at 8000×6000 (48 MP, below) + limit-guard behavior at large sizes |
| Binary size | 3.22 MiB (3,375,376 bytes) release build with LTO + `strip = "symbols"` — deterministic: identical byte count from a fresh clone |
| Clean-machine test | fresh clone of `main` (235f416), cold `CARGO_TARGET_DIR`: `cargo build --release` 35.5 s wall, then `cargo test --release` **94/94 pass** |
| Documentation | `docs/cli.md` metadata section rewritten for write-through; README expanded for the release; `docs/dependency-licenses.md` current (MIT/Apache-2.0-compatible BFS-3 transitive set, unchanged this phase except `flate2`) |
| Release prep | release notes for v1.0.0 (`docs/release-notes-v1.0.0.md`); manifests build via the existing `tooling` manifest script (aarch64-apple-darwin + universal2-apple-darwin verified for rc.1) |

## Defects found by fuzzing (all fixed, with regression tests)

1. **GIF inspect could allocate 17 GB from a 20-byte file.** `inspect`
   claimed to be structural but for GIF it composites every frame onto the
   full logical-screen buffer, and `GifDecoder::new` sets `no_limits()`.
   A header declaring 65535×65535 crashed the CLI with a 17 GB `calloc`
   *before* the engine's `--max-pixels-mp` guard ran. Fixed twice over:
   a 64 M-pixel header-level safety ceiling in `codec-gif` (checked before
   any decoder alloc, exit 4) and explicit `image::Limits` (512 MiB
   `max_alloc`) passed down to the decoder so crafted frame rectangles are
   refused inside the decoder as well.
2. **Pixel-budget checked after decode.** The engine's `--max-pixels-mp`
   check ran after the full decode; now the declared-geometry budget is
   enforced immediately after `inspect`, before any decode work — a 48 MP
   image with `--max-pixels-mp 20` now refuses in 0.05 s.
3. **`expect("one frame")` panic in GIF decode.** A GIF whose only frame
   fails to composite produced an empty frame list and a `panic!` (abort
   under the fuzzer). Now a `corrupt_input` error. The same pattern was
   hardened defensively in `engine.rs` for the `--flatten` and
   single-frame-animation routes.

Regression tests: `gif_header_oversized_canvas_fails_fast` (e2e, exit 4 +
`resource_limit_exceeded` JSON code), `oversized_canvas_is_a_resource_
limit_before_compositing` (codec unit test), and `gif_bad_palette.gif`
regenerated as a genuine bad-palette fixture (it had itself been the
17 GB-header PoC).

## Fuzz results

| target | execs | wall | result |
| --- | --- | --- | --- |
| `webp_container` | 13,994,630 | 241 s | clean (0 crashes, 0 OOM, 0 timeouts) |
| `sniff_extract_decode` | 103,415 | 241 s | clean after the fixes above (slower per exec: includes full decode + encode round-trips) |

Honest scope: bounded smoke runs on one machine, not a full fuzzing
campaign. The targets build with `cargo +nightly fuzz build` and are
excluded from the main workspace so nightly/sanitizer flags never leak
into normal builds or CI. Longer unattended runs and OSS-Fuzz-style
continuous fuzzing remain future work.

## Performance (8000×6000 = 48 MP, macOS aarch64, release)

| conversion | wall | output |
| --- | --- | --- |
| png → webp (lossy, q75 default) | 1.55 s | 908 KB |
| png → webp q90 | 1.71 s | 1.40 MB |
| png → webp `--method 6` | 64.5 s | 909 KB |
| png → webp `--lossless` | 6.9 s | 140 KB |
| png → jpeg (`--background`, alpha flattened) | 0.83 s | 30.5 MB |
| png → tiff (uncompressed) | 0.42 s | 192 MB |
| webp → png (round-trip) | 0.45 s | 43 MB |

No pathological behavior at 48 MP. `--method 6`'s exhaustive search is the
one expected slow path (≈40× the default); documented in the release notes
as guidance, not a defect. The 48 MP default guard (`--max-pixels-mp 100`)
passes; lowering the guard refuses in 0.05 s before decode (above).

## Test and build inventory

- 94 tests, all green in release on this machine and from a clean clone:
  core 18, codec-gif 1, codec-webp mux 2, cli unit 7, e2e integration 12,
  matrix 7, streams 22, advanced 15, metadata 10.
- `cargo clippy --release --all-targets -- -D warnings`: clean.
- `cargo fmt --all`: clean.
- Exit-code contract (2 usage / 3 unsupported / 4 limit / 5 output /
  6 internal) unchanged and covered.

## Recommendation for v1.0.0

`main` at 235f416 is the v1.0.0 candidate. Bump the two `0.1.0` versions in
`Cargo.toml`/`Cargo.lock` (or keep 0.1.0 per the caret-version convention
already used for rc.1) and tag; the manifest script and clean-machine flow
are the same as rc.1's, already verified.
