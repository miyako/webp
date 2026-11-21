# ADR-001 — Language and workspace structure

- **Status:** Accepted (proposal validated by feasibility sprint, 2026-11-21)
- **Context:** Master plan §3.1 recommends Rust; T04 required a proof.
- **Decision:** Rust, cargo workspace with per-format codec crates under
  `crates/` and a `cli` binary crate, per master plan §21.
- **Alternatives considered:** C with libwebp (unsafe/unmaintainable at
  required discipline level); Swift (no Windows story); pure-Rust-only
  codecs (see ADR-002).
- **Consequences:** Native builds are rustc-driven; Windows MSVC target
  needs a Windows runner for verification. Workspace split keeps codec
  boundaries enforced by cargo.
- **Validation evidence:** Host toolchain Rust 1.100 builds the prototype
  and produces a runnable macOS Universal binary (see
  `docs/feasibility-report.md`).

# ADR-002 — WebP codec/backend

- **Status:** Accepted (evidence-based)
- **Decision:** Backend is `libwebp` 1.5.0 via `libwebp-sys2`
  (vendored C, statically compiled; BSD-3-Clause), wrapped by the safe
  crates `webp` 0.3 for still images and `webp-animation` 0.10 for
  animation. Lossy + lossless + alpha + animation all proven working from
  Rust.
- **Alternatives:** Pure-Rust WebP codecs exist (e.g. the `image` crate's
  `image-webp`) but do not provide lossy *encoding* today; decoder-only
  parity is insufficient for the P0 capability matrix. A pure-Rust
  fallback encoder remains a backlog option.
- **Critical configuration constraint (prototype-proven):** libwebp
  lossless encoding with `exact=0` (its default) silently rewrites the RGB
  values of fully-transparent pixels; only alpha survives. The engine MUST
  set `exact=1` whenever lossless output must be pixel-exact, and SHOULD
  emit a diagnostic when lossy encoding flattens RGB under transparency.
- **Consequences:** `Cargo.lock` pins native source; license inventory
  must include libwebp's BSD-3-Clause notice at distribution time.
- **Validation evidence:** `tooling/codec-prototype/` 14/14 checks pass;
  lossless round-trip pixel-exact *only* with `exact=1`.

# ADR-003 — Format scope and priorities

- **Status:** Accepted (aligns with master plan §4)
- **Decision:** P0 read/write: WebP, PNG, JPEG, GIF, BMP. P1: TIFF,
  animation, metadata. P2: ICO, TGA, PNM, QOI, batch. P3 (research):
  AVIF, JXL, APNG, HEIC, SVG, RAW. `image` 0.25 covers P0 + TIFF decode/
  encode for stills today (prototype-proven PNG/JPEG/TIFF/BMP; JPEG has no
  alpha as expected).
- **Consequences:** Capability registry, not documentation, reports
  actual support.

# ADR-004 — Internal image/frame model

- **Status:** Accepted (proposal)
- **Decision:** Internal model is a `StillImage` or `FrameSequence`
  abstraction carrying dimensions, pixel data (RGBA8 first-class), alpha
  flag, ICC, EXIF/XMP containers, orientation, loop count, per-frame
  (data, offset, blend, disposal, duration), source format and bit-depth
  provenance. A still is a one-frame sequence. Codecs translate
  to/from this model; the policy layer only sees the model.
- **Alternatives:** Passing each codec's native frame types through the
  pipeline (rejected: leaks codec detail; master plan §3.5).
- **Consequences:** Animation-ready from day one; 16-bit/CMYK inputs are
  normalized by a documented, diagnosable step in P1.

# ADR-005 — CLI syntax and protocol

- **Status:** Accepted (draft contract; freeze at first tag)
- **Decision:** Commands and options exactly as master plan §6
  (`convert` default via bare `INPUT OUTPUT`, `inspect`, `formats`,
  `capabilities --json`, `--version`). JSON protocol schema version
  starts at `1` only after structure is validated; development builds may
  emit `"protocol": 0`. Exit codes per §7.5 (0/2/3/4/5/6/7). All four
  file/stdin × file/stdout combinations are P0; `-` selects stdio; binary
  I/O on Windows must use O_BINARY-equivalent handling.
- **Testing gate:** `cargo test` includes a protocol-conformance suite
  (`tests/protocol/`) run for every PR (master plan WP-17).

# ADR-006 — Binary stdout/stderr separation

- **Status:** Accepted
- **Decision:** When output is `-`, stdout carries image bytes and *only*
  image bytes; JSON reports go to stderr in that mode. With file output,
  JSON goes to stdout (a dedicated `--report <file>` may follow).
  Diagnostics default to stderr; `--quiet` suppresses only info-level.
- **Validation evidence:** Prototype verifies byte-exact outputs exist;
  contamination tests are a Phase 4 gate.

# ADR-007 — Alpha and flattening policy

- **Status:** Accepted
- **Decision:** Preserve alpha where the target supports it. JPEG (and
  any RGB-only target) requires explicit `--background` (or the user
  rejects) — no silent flattening; alpha loss always produces a
  diagnostic. Lossless WebP must set `exact=1` (ADR-002). GIF
  transparency is limited to 1-bit by the format; documented, never
  silent.
- **Validation evidence:** Prototype: lossy+alpha preserves the ~16k
  transparent pixels; JPEG output drops alpha (5920B, 0 transparent).

# ADR-008 — Metadata policy

- **Status:** Accepted (proposal)
- **Decision:** `--metadata all|icc|none` with `all` as default in v0,
  documented as "preserve what the target supports" and *never silent*:
  every dropped field/category (EXIF, XMP, ICC, comments, timestamps)
  yields a `metadata_dropped` diagnostic (severity warning). v0 baseline:
  read EXIF (orientation) via `kamadak-exif`; full write-through of ICC
  into WebP/PNG is P1 work gated by codec support.
- **Consequences:** Capability registry exposes per-format metadata
  support so callers can preflight.

# ADR-009 — Orientation and color handling

- **Status:** Accepted (proposal)
- **Decision:** EXIF orientation (all 8 values) is applied to pixels only
  when explicit `--orient auto|none` says so; default for v0 is `none`
  (pass through, report orientation in `inspect`), because auto-rotation
  silently changes stored dimensions. Color policy: preserve ICC when the
  target supports it; grayscale/16-bit inputs normalizable to 8-bit with
  a diagnostic; CMYK JPEG decodes to RGB with a diagnostic. No claim of
  color management beyond documented behavior.
- **Testing:** All 8 EXIF orientations in the fixture corpus
  (`tests/fixtures/metadata/orientation/`).

# ADR-010 — Animation semantics and release scope

- **Status:** Accepted (proposal; dedicated milestone)
- **Decision:** Animation is P1, shipped as its own milestone per master
  plan §16 Phase 5 — but the frame model is architecture-ready in P0
  (ADR-004). Behavior until animation support ships: detect animation,
  report frame count, refuse conversion to same-data-loss targets unless
  `--frame` or `--flatten` is explicit. Never silently flatten.
- **Validation evidence:** Prototype proves animated GIF decode (gif
  crate, frames + delays), animated WebP decode + encode (webp-animation,
  3 frames, timestamps), and still-vs-animated detection
  (`WebPBitstreamFeatures.has_animation`).

# ADR-011 — Resource limits and security

- **Status:** Accepted (defaults to be benchmarked; limits land in Phase 4)
- **Decision:** Pre-decode defense: input byte cap (`--max-input-bytes`),
  header-validated pixel cap (`--max-pixels`), width/height caps, frame
  caps (`--max-frames`), total frame pixels, processing timeout. Spam
  behavior: any limit breach → exit 4 with structured
  `resource_limit_exceeded` error, no partial output ever committed.
  Fuzz corpus under `tests/fuzz/`; malformations from the fixture corpus
  must never panic, only exit 3. Dependencies pinned via `Cargo.lock`
  and `rust-toolchain.toml`; `cargo audit` in CI.
- **Rationale:** Decoders accept attacker-controlled dimensions; libwebp
  is C and must never see unbounded inputs.

# ADR-012 — Repository strategy

- **Status:** Amended 2026-11-21 (single repository, two release tracks)
- **Decision (amended):** One repository. This repo (`webp`, GitHub
  `miyako/webp`) hosts both the CLI and the 4D component. Two release
  tracks, independent version ladders:
  - **CLI releases** — CLI's own convention: `v`-prefixed SemVer tags
    (`v0.1.0-rc.1`, `v1.0.0`), assets named `webp-convert-<version>-<target>.zip`.
  - **4D component releases** — Dependency-manager contract: prefix-free
    SemVer tags **without** `v` (first: `20.0.0`), asset exactly
    `webp.zip` (repository basename), one released component per tag.
    Component versions are entirely separate from CLI versions.
  The component pins CLI release assets in `4d/cli/asset-lock.json`
  (URL + tag + source commit + SHA-256 of ZIP/manifest/binary), following
  the audited `miyako/quickdraw` and `4d-component-template` conventions;
  the component ships the CLI **preinstalled** under
  `Resources/bin/{macOS,Windows}/`.
- **Was:** Two repositories (reverted 2026-11-21: the Dependency-manager
  asset name must equal the repository basename, and a second repo named
  `webp` is not available while the CLI repo holds that name; user
  decision).
- **Alternatives:** Separate component repository (would require renaming
  the CLI repo to `webp-convert` first — rejected for now).

# ADR-013 — 4D executable packaging and asset pinning

- **Status:** Accepted (proposal; Codename gates at WP-21)
- **Decision:** Component bundles per-platform binaries at
  `Resources/bin/{macOS/webp-convert,Windows/webp-convert.exe}` selected
  by target name (`macOS-universal` / `Windows-x64`), like quickdraw's
  `Converter.new($target)`. `asset-lock.json` pins release URL, tag,
  source commit and SHA-256 for every ZIP, manifest and executable;
  component startup verifies the hash of the resolved binary before the
  first launch (fail fast, no PATH search, no runtime download, no shell).
- **Validation evidence:** Template + quickdraw audit (AGENTS.md
  conventions, `_Process.4dm` SystemWorker pattern, host smoke tests,
  tool4d compiler project). macOS Universal build verified locally;
  binary depends only on system `libSystem`/`libiconv`.

# ADR-014 — Blob integration (4D)

- **Status:** Deferred pending WP-23 feasibility
- **Decision:** File-mode API first (master plan §12.4). Blob mode is
  gated on a SystemWorker prototype proving: binary stdin/stdout with
  `dataType`, explicit stdin close, large-Blob behavior, timeouts, memory
  ceilings, and byte integrity. If unreliable, file mode ships alone.
- **Note:** quickdraw's binding contract documents *no* stdin-image API
  shipped initially — consistent with this gate.

# ADR-015 — Native build and release process

- **Status:** Accepted (proposal)
- **Decision:** CI builds native targets: macOS aarch64 + x86_64
  (lipo → Universal, verify `lipo -info`, `otool -L`, sign/notarize/staple
  with Developer ID when credentials exist) and Windows x86_64-msvc on a
  Windows runner (verify `dumpbin /dependents`, static CRT, binary-safe
  stdio). Releases publish a manifest (version, commit, targets, libwebp
  version, build date, SHA-256, dependency + license inventory,
  capability manifest). Reproducibility: pinned toolchain, `Cargo.lock`,
  fixed libwebp source.
- **Validation evidence (local, 2026-11-21):** arm64 + x86_64-apple-darwin
  release builds of the prototype link successfully; `lipo -create`
  produces `x86_64 arm64` Universal; `otool -L` shows only
  `/usr/lib/libiconv.2.dylib` and `/usr/lib/libSystem.B.dylib`; the
  Universal binary passes the 14-check self-test. **Windows x64: NOT
  verified on this machine — requires a Windows CI runner before any
  release claim.**

# ADR-016 — Hosted acceptance and Apple-signed component releases

- **Status:** Accepted (pipeline implemented, ported from quickdraw; signing
  exercised to the fail-closed certificate boundary locally — full Apple
  path requires the GitHub secrets and will be proven on the first hosted
  run)
- **Decision:** Port the audited `miyako/quickdraw` hosted-CI + Apple-signing
  pipeline to this repository, re-pointed at `4d/` paths and the webp asset
  lock:
  - `.github/workflows/4d.yml` — reusable hosted acceptance job (macOS ARM64
    runner): SHA-256-pinned tool4d download (`ci_config.py download-tool4d`),
    pinned CLI asset download + staging, source host test, tool4d compile,
    packaging, compiled-host test on the final archive, then per-policy
    Apple signing + notarization (`sign_release.py`), with a compiled-host
    re-test on the signed ZIP. Evidence: `dist/webp.acceptance.json`.
  - `.github/workflows/4d-build.yml` — manual dispatch on `main`: preflight
    (immutable commit, `ci_config.py validate --release`, tag availability,
    secret-free signing policy read), 4D acceptance, then
    `release_assets.py --create-tag --publish` for the fail-closed
    verified, prefix-free tag + `webp.zip` release.
  - `.github/workflows/ci.yml` runs the unsigned acceptance per push/PR,
    gated on repository variable `FOURD_ENABLED=true`.
  - Signing is gated two ways and upgrade-only: the component version must
    appear in `4d/release-policy.json` → `apple_notarized_versions`, or the
    caller passes `--require-apple-signing`. Secrets
    (`APPLE_DEVELOPER_ID_CERTIFICATE{,_PASSWORD}`, `KEYCHAIN_PASSWORD`,
    `NOTARYTOOL_{APPLE_ID,TEAM_ID,PASSWORD}`, optional `CODESIGN_APPLE_ID`)
    are installed by the user-run `4d/scripts/set-release-secrets.sh`
    helper; agents never handle them.
  - tool4d pin precedence: repository variables `TOOL4D_URL` +
    `TOOL4D_SHA256` (primary), falling back to the official
    `resources-download.4d.com` / `builds2.4d.com` download verified against
    the package pin when a `tool.sha256` exists.
- **Alternatives:** Local-only release (rejected: no Apple notarization or
  hosted evidence possible from this machine); per-run ad-hoc scripts
  (rejected: quickdraw's audited, fail-closed scripts are the reviewed
  precedent).
