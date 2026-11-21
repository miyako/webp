# Phase 5 report — advanced semantics

Status: **complete for the v0 gate** (plan "Phase 5": metadata policies,
16-bit/CMYK behavior, animation detection/frame handling/animation output,
TIFF pages, diagnostics). Gate: *no silent loss of significant data* — every
page, frame, and bit-depth reduction is now either preserved, explicitly
selected, or loudly diagnosed. Verified on macOS aarch64 in release; Windows
x64 / macOS x64 verification continues to be delegated to CI (native jobs).

## What shipped this phase

| piece | state |
| --- | --- |
| TIFF `--page N` (plan §5.4) | IFD-chain page walker via the direct `tiff` crate (the `image` crate exposes no page API); Gray/GrayA/RGB/RGBA/Palette/CMYK at 8/16-bit; 16→8 normalized `(v*255+32767)/65535`; page count already reported since Phase 3 |
| Multi-page refusal → explicit selection | multi-page TIFF without `--page` exits 3 (was: v0 refuse without escape hatch); `--page` + `--frame` combined exits 2 |
| 16-bit TIFF | normalization now diagnosed `bit_depth_reduced` (info) via a BitsPerSample header probe (core::metadata) |
| Animated output (plan §5.3) | webp→webp and gif→webp (frame count + timing preserved, lossy/lossless), webp/gif→gif (`1/100 s` delay quantization documented); animated input without flags to an animation-capable target now *works* instead of refusing |
| `--flatten` | functional: first frame, loud `animation_flattened` warning (was: refused in v0) |
| Animated→still refusal | animated input with no selection flags to a still target exits 3, message names the three escapes (`--frame N`, `--flatten`, animation-capable format) |
| Engine routing rework | `decode_input` routes `--page`/`--frame`/whole-decode; `run_convert` owns the Route::{Still,Animated} decision so refusals know the *target*; pixel budget checks the animation canvas |
| `tiff` crate as workspace dep | decode-only use in codec-tiff (Apache/MIT, dual-licensed); recorded in the dependency-license bookkeeping |

New tests: `crates/cli/tests/advanced.rs` — 15 tests: multipage refusal,
per-page pixels (red page 0, blue page 1), out-of-range page (exit 2),
page+frame conflict, single-page path unchanged, inspect page count,
stdin `--page`, 16-bit diagnostic, webp→webp frame+timing round-trip,
webp→gif frame count + 10 ms granularity, animated→png refusal +
no-litter, `--flatten` first-frame byte equality + diagnostic,
`--frame 1` byte equality, frame out-of-range, gif→webp frames + timing.
Fixtures added via a committed generator (same pattern as phase 3):
`crates/codec-tiff/examples/write_fixtures.rs` writes
`two_pages.tiff` (red/blue pages) and `gray16.tiff`.

Total suites: core 18, cli unit 7, e2e 11, matrix 7, streams 22,
advanced 15 = **80 tests**.

## Rules now pinned by tests

- Multi-page TIFF: refuse without `--page` (exit 3); decode the requested
  page exactly; out-of-range page is usage (exit 2).
- `--page` and `--frame` are mutually exclusive (exit 2).
- 16-bit input: exit 0 but `bit_depth_reduced` diagnostic present.
- Animation: frames + per-frame timing round-trip webp→webp;
  webp→gif keeps both frames with 10 ms granularity; still target without
  an explicit selection is refused; `--flatten` yields *exactly* frame 0
  plus the warning.

## Not done this phase (honest list)

- **Metadata write-through** (ICC/EXIF/XMP chunks copied into webp output):
  deferred to Phase 6. `--metadata` policies already behave (strip /
  diagnose), drops are loudly diagnosed (`metadata_dropped`), so the
  phase gate holds; write-through is an enhancement, not a loss fix.
- Per-frame alpha compositing/disposal differences beyond the existing
  composited-frame model (gif/webp decoders both yield composited frames).
- CMYK → ICC-profile color management (naive 1-minus conversion, documented
  in caps + restrictions).
- GIF animation loop_count other than infinite is not written on output
  (decode reads it where the format stores it; gif output always infinite).

## Verification evidence (this machine)

- `cargo test --release`: all suites green (80 tests, ~14 s).
- `cargo fmt --all` clean; `cargo clippy --release --all-targets -D
  warnings` clean.
- Manual smoke: page pixel colors verified through PNG decode; animated
  webp→webp re-inspected (2 frames); flatten output compared byte-for-byte
  to decoded frame 0.

## Next

Per the approved release plan: tag **v0.1.0-rc.1** with the manifest
(`scripts/release-manifest.sh`), after the Phase 6 fuzz/clean-machine
hardening gates v1.0.0. (rc.1 was already queued behind Phase 5.)
