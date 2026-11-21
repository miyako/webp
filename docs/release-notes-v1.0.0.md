# webp-convert v1.0.0

`webp-convert` is a portable, loud-by-default image conversion CLI: 6
formats in (WebP, PNG, JPEG, GIF, TIFF, BMP), 6 out, with honest
diagnostics instead of silent losses, a JSON report protocol, hard
resource limits, and a 3.2 MiB self-contained binary.

## Highlights since v0.1.0-rc.1

### Metadata write-through for WebP targets (v1)

- ICC profiles, EXIF, and XMP are extracted from JPEG (APP1/APP2),
  PNG (iCCP / eXIf / iTXt), WebP (RIFF chunks), and TIFF (tags 0x8773 /
  0x02BC, IFD0 EXIF) inputs and re-embedded into WebP output.
- `metadata_carried` info diagnostic per conversion; categories the
  target cannot store are dropped with a loud `metadata_dropped` warning.
- `--metadata icc|all|none` selects what to carry; `none` strips
  explicitly with per-category info diagnostics.
- Rotating pixels (`--orient auto`) rewrites the carried EXIF orientation
  to 1, so downstream viewers never rotate twice.
- Non-WebP targets still drop metadata (with warnings) — write-through to
  other formats is future work; comments/timestamps are not carried.

### Hardening driven by fuzzing

- Two libfuzzer targets (`fuzz/`) now cover sniffing, inspection, decode,
  encode round-trips, metadata extraction, and the RIFF muxer.
- Fixed: a 20-byte GIF could make `inspect` allocate 17 GB (frame
  compositing with decoder limits disabled) — added a 64M-pixel safety
  ceiling plus `image::Limits` (512 MiB) in the GIF codec.
- Fixed: the `--max-pixels-mp` / `--max-frames` budgets are now enforced
  immediately after header inspection, before any decode work.
- Fixed: a corrupt single-frame GIF crashed via an `expect` panic; it is
  now a regular `corrupt_input` error.
- `cargo audit`: 0 vulnerabilities across 104 dependencies.

### Known limitations (honest list)

- Metadata write-through: WebP targets only.
- APNG input decodes to its first frame.
- 16-bit PNG/TIFF normalize to 8-bit (diagnosed).
- CMYK TIFF pages convert with a naive 1-minus mapping (diagnosed).
- GIF output quantizes to 256 colors, 1-bit transparency (diagnosed).
- `--method 6` on very large images is slow by design (exhaustive
  WebP encode search; ~40× the default time at 48 MP).

## Verification

- 94 automated tests green; `cargo clippy -D warnings` clean.
- Fuzz: 14.0M + 103k execs in bounded runs, clean after the fixes above.
- Clean-machine check: fresh clone, cold build 35 s, all tests pass.
- Second-architecture / second-OS verification continues in CI
  (Windows x64, macOS x64, Ubuntu x86-64).

## Upgrading from v0.1.0-rc.1

No breaking changes to flags, exit codes (2/3/4/5/6), or the JSON
protocol (version 1). New diagnostics: `metadata_carried`,
`metadata_stripped`, `orientation_applied`. GIF inputs with absurd
declared canvases now fail with exit 4 (`resource_limit_exceeded`)
instead of attempting the conversion.
