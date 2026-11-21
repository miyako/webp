# Feasibility Report — Phase 0/1 (Gates G0/G1)

> Date: 2026-11-21 · Machine: macOS 27.2.1 (arm64) · Toolchain: Rust 1.100.0
> Scope: kickoff tasks T01–T10 from master plan §22. Evidence-producing
> prototype: `tooling/codec-prototype/` (scratch tool, not a product artifact).

## How to reproduce

```sh
cd tooling/codec-prototype
cargo run                     # 14-check codec/document matrix
cargo test lossless_mismatch_analysis -- --nocapture
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create ... -output out/universal/codec-prototype
otool -L out/universal/codec-prototype && ./out/universal/codec-prototype
```

## T01 — Scope and v1 format matrix

Unchanged from master plan §4/§16; recorded as ADR-003. No open decision
blocks implementation start (Gate G0, modulo items flagged below).

## T02 — 4D component template audit

Local repo `~/Documents/GitHub/4d-component-template` audited. Conventions
confirmed: `component/<name>/Project/Sources/Classes/` with `_CLI.4dm`,
`_Process.4dm` (SystemWorker lifecycle), `Greeting.4dm` example API;
`Resources/bin/` for executables; tool4d compiler project under
`tooling/compiler/`; host smoke tests with `contract.json`;
`tests/cli-contract.json`; identity rules (template.json/repository,
package name, component dir case-exact); release tags for components carry
no `v` prefix. Template ships Rust/Go/C++ CLI starters (`cli-starters/rust/`
includes a pinned `.cargo/config.toml` and license audit layout).

## T03 — quickdraw release-architecture audit

Local repo `~/Documents/GitHub/quickdraw` is a working predecessor of
exactly this arrangement: a CLI-backed 4D component whose
`cli/asset-lock.json` pins an independently released CLI (`quickdraw-cli
v0.1.0`, source commit, release ZIP URLs, SHA-256 of ZIP/manifest/executable).
Its AGENTS.md encode the rules the master plan restates (no PATH search, no
shell, no second compiler, no re-lipo of signed binaries, 30 s process
timeout, closed stdin, no-clobber enforced by the CLI, structured result
model). The webp component must copy these conventions; the plan's §12/§13/ADR-013
match observed reality.

## T04 — WebP codec candidates (Rust)

A 14-check matrix ran green; the important results:

| Check | Result | Evidence |
| --- | --- | --- |
| libwebp lossy encode/decode (RGB) | PASS | 742 B output, PSNR ≈ 46.2 dB |
| libwebp lossy encode/decode (RGBA) | PASS | transparency preserved (16 129 px) |
| libwebp **lossless** RGBA round-trip | PASS **only with `exact=1`** | 710 B; pixel-exact |
| libwebp lossless default (`exact=0`) | documented hazard | RGB of fully-transparent pixels rewritten (all 16 129 changed pixels had alpha=0) |
| quality knob | PASS | q90 1 210 B vs q30 554 B |
| still-vs-animated detection | PASS | `WebPBitstreamFeatures.has_animation` |
| PNG round-trip with alpha | PASS | pixel-exact, 65 704 B |
| JPEG has no alpha (lossy) | PASS (expected data loss) | 5 920 B, 0 transparent px after flatten to RGB |
| TIFF round-trip with alpha (`image`) | PASS | 262 358 B, pixel-exact |
| BMP round-trip | works (info) | 262 266 B; alpha-in-BMP stays a documented caveat |
| animated GIF decode (3 frames + timing) | PASS | `gif` 0.14 crate |
| animated GIF frames via `image` | PASS | `GifDecoder::into_frames()` |
| animated WebP decode (3 frames + timestamps) | PASS | `webp-animation` 0.10 |
| animated WebP encode | PASS | 258 B, timestamps accumulate (600 ms total) |

Canonical engine rules derived (captured in ADR-002/ADR-007):

1. Lossless WebP must set `exact=1`, or RGB under fully-transparent
   pixels is silently rewritten — a plan-level "no silent data loss"
   violation if left at libwebp defaults.
2. Lossy WebP preserves perceived (premultiplied) transparency but is
   lossy in RGB; alpha channel survives, RGB of transparent px may change
   unless lossless/exact.
3. JPEG targets strip alpha — policy requires explicit `--background`.

Backend choice: libwebp 1.5.0 via `libwebp-sys2` 0.2.0 (vendored C, static)
+ `webp` 0.3.1 (still) + `webp-animation` 0.10.0 (animation). Pure-Rust
WebP lacks lossy *encoding* today → not sufficient as primary backend
(ADR-002).

P0 sibling codecs via `image` 0.25 (PNG/JPEG/GIF/BMP) verified enough for
a fix-forward decision in Phase 3; TIFF verified for the still case.

## T05 — Animation capabilities and semantics

Decode and encode paths for animated GIF and animated WebP are proven
(quantity, per-frame timing). Blend/disposal compositing and loop-count
round-trips were **not** exercised in the prototype; they are the P1
animation milestone's first test targets (ADR-010). Detection of animated
WebP is available pre-decode (`has_animation`), which is what refusal
semantics ("never silently flatten") need.

## T06 — Dependency/license audit

`cargo-license` over the prototype's dependency tree: **52 crates**.

- Licenses: MIT / Apache-2.0 / BSD-2/3-Clause / Zlib / 0BSD /
  Unicode-3.0 — all permissive, redistribution-compatible with MIT CLI.
- `r-efi` offers LGPL only among its choices and compiles only on UEFI
  targets; it is not linked into macOS/Windows binaries.
- Vendored native C: **libwebp 1.5.0**, BSD-3-Clause (Google) — its
  `COPYING` + `PATENTS` must ship in the license inventory of releases and
  in component `Resources/licenses/`.
- Build-time deps compile C from vendored sources (no system lookup
  required for portability).

Full inventory snapshot: re-run
`~/.cargo/bin/cargo-license` at release time (CI job, WP-03).

## T07 — Windows x64 portable build prototype

**NOT verified — cannot be on this machine.** `x86_64-pc-windows-msvc`
compilation requires either a Windows runner or `cargo-xwin`; neither was
exercised. Honesty rule (plan §2.10): Windows remains an *unverified
platform* until a native CI job runs the conversion matrix there. This is
the only red item in the sprint; it does not block CLI design work.

## T08 — macOS Universal build prototype

| Step | Result |
| --- | --- |
| `cargo build --release` (aarch64-apple-darwin) | PASS, Mach-O arm64 |
| `cargo build --release --target x86_64-apple-darwin` | PASS, Mach-O x86_64 |
| `lipo -create … -output` | PASS — `Architectures in the fat file: x86_64 arm64` |
| `otool -L` (both slices) | only `/usr/lib/libiconv.2.dylib` + `/usr/lib/libSystem.B.dylib` |
| Universal binary executes self-test | PASS (all checks) |

Fully self-contained for stock macOS; no codec dylibs; signing/notarization
still required before distribution (needs Developer ID credentials —
release-time item, not a feasibility blocker).

## T09 — 4D SystemWorker Blob communication prototype

**Not yet performed** (not runnable from this repo). Blocked only on
scheduling; quickdraw's shipped component gives high confidence for file
mode. Blob mode stays gated per ADR-014 and plan §12.5. A prototype
should run a Rust binary through tool4d + SystemWorker with `dataType`
binary stdin/stdout and byte-integrity checks before Phase 8.

## T10 — Architecture and CLI protocol ADR

ADR-001…ADR-015 written (`docs/adr/ADRs.md`), all grounded in the evidence
above. CLI contract (commands, options, JSON protocol, exit codes) is
drafted in ADR-005 and must be exercised by `tests/protocol/` once
implementation starts; contract freeze happens at the first tagged release.

## Gate status

- **G0 (no unresolved decision that materially affects implementation):**
  **PASS** — language, backend, scope, model, protocol, packaging are
  decided (ADRs). Open but non-blocking items: Blob prototype (T09),
  Windows runner (T07), signing credentials.
- **G1 (required codecs + portable builds demonstrated):** **PASS for
  macOS; Windows pending** — codecs fully demonstrated; portability
  demonstrated for macOS Universal with system-only dylibs. Gate G1 will
  be fully green when a Windows x64 native run of the conversion matrix
  is recorded.

## Deliverable files

- `docs/plan/webp-convert-master-plan.md` — authoritative blueprint copy.
- `docs/adr/ADRs.md` — ADR-001…015.
- `tooling/codec-prototype/` — evidence prototype (scratch, never ships).
- `tests/fixtures/` — permanent corpus skeleton (contents land in WP-16).

## Known limitations / honesty notes

- macOS native tests ran on arm64; the x86_64 slice built but ran only
  implicitly through the Universal self-test (Rosetta) — an explicit x64
  runner run is still required by plan §16 for release claims.
- Prototype exercised synthetic gradients, not EXIF/ICC/XMP fixtures;
  metadata write-through capability remains unproven (P1).
- Three libwebp bindings coexist in the prototype tree (`webp`→
  `libwebp-sys` 0.9.6; `webp-animation`→`libwebp-sys2` 0.1.11+0.2.0),
  compiling libwebp more than once. The product workspace must converge on
  a single sys crate (plan §3.5 isolation).
