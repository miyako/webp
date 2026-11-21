# WebP Converter CLI + 4D Component --- Master Project Plan

> **Status:** Proposed master specification\
> **Purpose:** Authoritative project blueprint for human developers and
> autonomous coding agents.\
> **Strategy:** Build a portable standalone image-conversion CLI first;
> integrate it into a 4D component second.\
> **Target platforms:** Windows x64 and macOS Universal (x86_64 +
> arm64).

------------------------------------------------------------------------

## 1. Project Vision

Build a portable, self-contained image conversion engine and CLI, then
package it as a thin 4D component.

The CLI is the product and source of truth for image conversion, codecs,
policies, safety, diagnostics, and behavior. The 4D component is an
integration layer only. Conversion logic must not be duplicated in 4D.

### Goals

-   Convert WebP, PNG, JPEG, GIF, TIFF, BMP, and other formats approved
    by the capability matrix.
-   Support file-to-file conversions.
-   Support binary stdin/stdout pipelines.
-   Provide lossy and lossless WebP modes.
-   Preserve alpha when the target format supports it.
-   Make metadata, orientation, color, animation, and multi-page
    behavior explicit.
-   Produce portable executables requiring no separately installed
    converter or codec.
-   Support Windows x64 and macOS Universal.
-   Expose a stable, versioned machine-readable protocol.
-   Integrate with 4D through a bundled executable.
-   Provide reproducible builds, checksums, dependency/license
    inventories, and CI.

### Non-goals for initial release

Unless explicitly promoted through a scope decision, exclude Linux,
Windows ARM, SVG rasterization, RAW processing, remote conversion
services, and advanced editing operations such as crop/resize.

------------------------------------------------------------------------

## 2. Non-negotiable Principles

1.  **CLI first:** The standalone CLI must be independently useful.
2.  **One conversion engine:** CLI and 4D must use the same engine.
3.  **No silent data loss:** Never silently discard animation frames,
    TIFF pages, alpha, metadata, orientation, or significant color
    information.
4.  **Portability is proven:** A successful build is not proof of a
    portable executable.
5.  **Contracts before parallel implementation:** Agents work against
    approved interfaces.
6.  **Capability-driven behavior:** Runtime capability information must
    reflect what the binary actually supports.
7.  **Safety by default:** Treat input images as untrusted and enforce
    configurable resource limits.
8.  **Release integrity:** The component must pin and verify the exact
    CLI asset it bundles.
9.  **Small, reviewable changes:** Implement and validate incrementally.
10. **Honest verification:** Never report a platform or test as verified
    unless it was actually run.

------------------------------------------------------------------------

## 3. Strategic Architecture

### 3.1 Recommended stack

-   **Language:** Rust.
-   **WebP backend:** Start with a feasibility prototype using
    `libwebp`; evaluate a pure-Rust alternative for comparison or
    fallback.
-   **CLI framework:** `clap`.
-   **Machine-readable protocol:** Versioned JSON.
-   **4D integration:** 4D SystemWorker, using the component template's
    conventions.
-   **CI/CD:** GitHub Actions.
-   **Codec implementations:** Use suitable Rust/native libraries behind
    isolated adapter interfaces.

These are recommendations, not assumptions. Phase 0/1 must validate the
stack, capabilities, build portability, dependencies, and licensing.

### 3.2 Repository strategy

Recommended two-repository arrangement:

1.  **CLI repository** --- application, codecs, conversion policies,
    protocol, tests, native builds, releases.
2.  **4D component repository** --- public API, executable resolver,
    SystemWorker integration, pinned CLI assets, component packaging,
    integration tests.

Keep repositories separate unless project maintainers deliberately
choose a monorepo before implementation.

### 3.3 CLI architecture

``` text
CLI
 ├─ Command parser
 ├─ Input resolver (file/stdin)
 ├─ Format detection
 ├─ Capability registry
 ├─ Conversion policy
 │   ├─ Alpha
 │   ├─ Metadata
 │   ├─ Orientation
 │   ├─ Animation
 │   └─ Color
 ├─ Codec adapters
 │   ├─ WebP
 │   ├─ PNG
 │   ├─ JPEG
 │   ├─ GIF
 │   ├─ TIFF
 │   └─ BMP
 ├─ Resource guards
 ├─ Output writer (file/stdout)
 └─ Diagnostics, errors, JSON protocol
```

### 3.4 Internal image model

Do not make the internal model exclusively a single static RGBA image.
Use an abstraction that can represent a still image or a sequence of
frames.

Model the information needed for conversion, such as:

-   Dimensions and format.
-   Color model and bit depth.
-   Alpha.
-   ICC profile.
-   EXIF, XMP, and other supported metadata.
-   Orientation.
-   Source details.
-   Frame data and timing.
-   Frame offsets.
-   Blend/disposal semantics.
-   Loop behavior.

A still image is a one-frame image. The architecture should be
animation-ready even if animation is released later.

### 3.5 Codec adapter interfaces

Keep codec-specific behavior isolated. The policy layer should not
depend on backend implementation details.

Each decoder/encoder adapter should expose appropriate operations, such
as:

-   Detect or identify.
-   Decode/encode.
-   Inspect.
-   Report capabilities.
-   Report limitations and version information.

Do not allow codec-specific details to leak throughout CLI argument
handling or 4D code.

------------------------------------------------------------------------

## 4. Scope and Format Capability Matrix

Treat this matrix as a target. The actual supported capabilities must be
verified and reported by the executable.

  Format                        Read                    Write Priority
  --------- ------------------------ ------------------------ ----------
  WebP                           Yes                      Yes P0
  PNG                            Yes                      Yes P0
  JPEG                           Yes                      Yes P0
  GIF                            Yes                      Yes P0
  BMP                            Yes                      Yes P0
  TIFF                           Yes                      Yes P1
  ICO                       Optional                 Optional P2
  TGA                       Optional                 Optional P2
  PNM                       Optional                 Optional P2
  QOI                       Optional                 Optional P2
  AVIF                      Research                 Research P3
  JPEG XL                   Research                 Research P3
  SVG         Out of scope initially   Out of scope initially P3

### Release priorities

**P0 --- Core**

-   WebP, PNG, JPEG, GIF, BMP.
-   Still image conversion.
-   File I/O and stream I/O.
-   WebP lossy/lossless.
-   Quality settings.
-   Alpha handling.
-   Format detection.
-   Structured errors.
-   Portable binaries.

**P1 --- Important**

-   TIFF.
-   Animated GIF and WebP.
-   Frame selection.
-   ICC.
-   EXIF orientation.
-   Metadata policies.
-   Multi-page TIFF policy.
-   4D Blob conversion, subject to feasibility.
-   Resource limits.
-   Fuzzing.

**P2 --- Expansion**

-   ICO, TGA, PNM, QOI.
-   Batch and recursive conversion.
-   Multi-page TIFF output.
-   Additional metadata controls.
-   Progress and cancellation.

**P3 --- Research**

-   AVIF, JPEG XL, APNG, HEIC, SVG rasterization, RAW.

Do not let P2/P3 items block a stable core release.

------------------------------------------------------------------------

## 5. Conversion Semantics

Before implementing a format, define what conversion means for it.
"Format supported" is not enough.

### 5.1 WebP modes

Support:

-   Lossy encoding.
-   Lossless encoding.
-   Quality control.
-   Compression effort/method.
-   Alpha quality.
-   Near-lossless mode if supported and justified.
-   Thread controls if appropriate.

Do not silently ignore options. Reject or diagnose options that do not
apply to a target codec.

Quality values are codec-specific. Do not claim identical visual meaning
across JPEG and WebP.

### 5.2 Alpha/transparency

-   Preserve alpha for formats that support it.
-   GIF palette/transparency limitations must be documented.
-   JPEG has no alpha.
-   JPEG conversion must require an explicit flattening background,
    unless a documented default is deliberately approved.
-   Alpha loss must produce a clear diagnostic.

Example:

``` sh
webp-convert --background "#ffffff" input.webp output.jpg
```

### 5.3 Animation

Animation must be architecture-ready from the beginning.

Rules:

-   Detect animation and report frame count.
-   Never silently drop frames.
-   Preserve animation when the source and target support it.
-   Refuse unsupported animated conversion unless the user explicitly
    selects flattening or a frame.
-   Define frame selection, timing, compositing, disposal, and looping
    semantics.
-   Test loop behavior and transparency.

Proposed explicit controls:

``` sh
webp-convert --frame 0 animation.gif still.png
webp-convert --flatten animation.webp still.jpg
```

Release animation in a dedicated milestone if it threatens core
stability. Do not silently flatten animated input.

### 5.4 TIFF

TIFF may contain multiple pages, high bit depth, varied compression,
color models, alpha, and metadata.

-   Detect and report page count.
-   Never silently discard pages.
-   Support explicit page selection or reject unsupported multipage
    input.
-   Define high-bit-depth conversion behavior.
-   Document compression and metadata limitations.

### 5.5 Color

Define behavior for RGB, RGBA, grayscale, palette images, CMYK, 16-bit
input, ICC profiles, and color-space conversion.

Recommended initial policy:

-   Preserve ICC profiles where supported.
-   Do not claim full color fidelity without testing.
-   Diagnose profile removal or unsupported conversion.
-   Document normalization of unsupported color representations.

### 5.6 Metadata

Define explicit metadata modes. Suggested semantics:

-   `all`: preserve compatible metadata when possible.
-   `icc`: preserve ICC only.
-   `none`: strip metadata.

Cover EXIF, ICC, XMP, orientation, comments, and timestamps. Report
unsupported or dropped fields where practical.

### 5.7 Orientation

Define:

-   Pixel rotation/normalization policy.
-   Whether orientation metadata is retained.
-   Stored versus display dimensions.
-   Behavior for all eight EXIF orientations.

Test all eight orientation values.

------------------------------------------------------------------------

## 6. CLI Contract

The CLI must be independently useful and stable.

### 6.1 Command structure

Provisional executable: `webp-convert`.

``` sh
webp-convert INPUT OUTPUT
webp-convert convert INPUT OUTPUT
webp-convert inspect INPUT
webp-convert formats
webp-convert capabilities --json
webp-convert --version
```

The default command should be conversion, so the short form works.

### 6.2 Examples

``` sh
# PNG → WebP
webp-convert photo.png photo.webp

# WebP → PNG
webp-convert photo.webp photo.png

# JPEG → WebP
webp-convert --quality 85 photo.jpg photo.webp

# Lossless WebP
webp-convert --lossless photo.png photo.webp

# WebP → JPEG with explicit flatten background
webp-convert --background "#ffffff" photo.webp photo.jpg

# Explicit output format
webp-convert photo.bin --format png output.png

# Inspect
webp-convert inspect photo.webp
```

### 6.3 Stream modes

Support all combinations:

  Input   Output   Required
  ------- -------- ----------
  File    File     Yes
  File    stdout   Yes
  stdin   File     Yes
  stdin   stdout   Yes

Use `-` for stdin/stdout.

``` sh
cat photo.png | webp-convert - --format webp > photo.webp
cat photo.webp | webp-convert - --format png > photo.png
cat photo.png | webp-convert - --format webp | another-tool
```

Windows must use binary stdin/stdout modes. Image bytes must not be
corrupted by text-mode behavior.

### 6.4 Proposed options

Implement options only when their associated feature is available.

  Option                Purpose
  --------------------- ----------------------------------
  `-i, --input`         Input file or `-`
  `-o, --output`        Output file or `-`
  `--from`              Explicit input format
  `--to`                Explicit output format
  `--format`            Output format override
  `-q, --quality`       Quality 0--100
  `--lossless`          Lossless WebP
  `--lossy`             Lossy WebP
  `--method`            Encoder effort
  `--alpha-quality`     Alpha compression quality
  `--near-lossless`     Near-lossless mode
  `--background`        Alpha flattening color
  `--metadata`          Metadata policy
  `--orient`            Orientation handling
  `--flatten`           Explicit animation flattening
  `--frame`             Frame selection
  `--page`              TIFF page selection
  `--max-pixels`        Pixel limit
  `--max-input-bytes`   Input byte limit
  `--max-frames`        Frame limit
  `--timeout`           Processing limit, where feasible
  `--overwrite`         Permit output replacement
  `--json`              Machine-readable result
  `--quiet`             Suppress informational messages

### 6.5 Format detection

Use image-content detection/magic bytes and decoder probing, not
extension alone.

-   Detect the actual format.
-   Warn about extension/content mismatch.
-   Reject unsupported or ambiguous inputs.
-   Avoid uncontrolled fallback chains.

### 6.6 Output safety

By default:

-   Do not overwrite existing files.
-   Write to a temporary output.
-   Validate successful encoding.
-   Commit atomically where supported.
-   Detect input/output aliasing.
-   Prevent replacement of input.
-   Consider symlink/path alias cases.
-   Clean up temporary files after errors.

Overwriting requires explicit opt-in.

------------------------------------------------------------------------

## 7. JSON Protocol and Exit Codes

A versioned machine-readable protocol is required for the 4D wrapper and
automation.

### 7.1 Stream separation

-   stdout is image data when output is `-`.
-   stderr carries diagnostics/reporting.
-   No banners, progress, logs, debug text, or color output on binary
    stdout.
-   For file output, JSON reporting may use stdout or a dedicated report
    destination, as defined by the finalized contract.

### 7.2 Success example

``` json
{
  "protocol": 1,
  "ok": true,
  "operation": "convert",
  "input": {
    "format": "png",
    "width": 1200,
    "height": 800,
    "frames": 1,
    "hasAlpha": true
  },
  "output": {
    "format": "webp",
    "mode": "lossless",
    "bytes": 154220
  },
  "diagnostics": []
}
```

### 7.3 Warning example

``` json
{
  "protocol": 1,
  "ok": true,
  "diagnostics": [
    {
      "code": "metadata_dropped",
      "severity": "warning",
      "message": "XMP metadata was not preserved."
    }
  ]
}
```

### 7.4 Error example

``` json
{
  "protocol": 1,
  "ok": false,
  "error": {
    "code": "unsupported_animation_target",
    "message": "Animated input cannot be converted to JPEG without explicit frame selection."
  }
}
```

### 7.5 Exit codes

Provisional stable taxonomy:

    Code Meaning
  ------ ------------------------------------------------
       0 Success
       2 Invalid arguments/options
       3 Corrupt/unreadable input or unsupported format
       4 Resource limit exceeded
       5 Output error
       6 Internal error
       7 Protocol error

Finalize before release and maintain compatibility.

------------------------------------------------------------------------

## 8. Capability Registry

Implement a central capability registry. Each format/backend combination
should report:

-   Read/write support.
-   Animation support.
-   Alpha support.
-   Metadata support.
-   ICC support.
-   Frame support.
-   Page support.
-   Bit-depth limitations.
-   Known restrictions.
-   Backend/codec version.

Power these commands and artifacts from the registry:

-   `formats`
-   `capabilities --json`
-   Documentation support tables
-   Test selection
-   4D compatibility checks
-   Release verification

The executable must report its actual capabilities---not static claims
copied from documentation.

------------------------------------------------------------------------

## 9. Security and Reliability

Treat all images as untrusted input.

### Required limits

-   Input bytes.
-   Pixel count.
-   Width/height.
-   Frames.
-   Total frame pixels.
-   Memory where feasible.
-   Processing time.

Choose default limits using benchmarks and realistic usage. Allow
overrides for legitimate large-image workloads.

### Required behavior

-   No panic on malformed inputs.
-   No silent fallback that hides invalid data.
-   No silent frame/page dropping.
-   No silent alpha flattening.
-   No silent metadata removal.
-   No accidental overwrite.
-   Stable structured failures.
-   Temp-file cleanup.

### Security testing

-   Decoder fuzzing.
-   CLI argument fuzzing.
-   Corrupt and truncated fixtures.
-   Oversized dimensions.
-   Malformed metadata.
-   Animation abuse cases.
-   Regression tests for crashes.

Pin and audit dependencies.

------------------------------------------------------------------------

## 10. Testing Strategy

### 10.1 Layers

1.  Unit tests --- argument validation, policy, capability registry,
    error mapping.
2.  Codec tests --- encoding/decoding, dimensions, color, alpha,
    metadata, animation.
3.  CLI integration --- paths, pipes, exit codes, binary output,
    overwrite safety.
4.  Platform tests --- Windows x64, macOS x64, macOS arm64.
5.  4D integration --- SystemWorker, file/Blob, protocol, packaging.
6.  Acceptance tests --- clean machine and released assets.

### 10.2 Permanent fixture corpus

``` text
tests/fixtures/
  static/
    png/
    jpeg/
    gif/
    webp/
    tiff/
    bmp/
  animated/
    gif/
    webp/
  alpha/
    full/
    partial/
    transparent/
  metadata/
    exif/
    icc/
    xmp/
    orientation/
  color/
    rgb/
    grayscale/
    palette/
    cmyk/
    16bit/
  malformed/
    truncated/
    corrupt/
    invalid_dimensions/
    oversized/
  multipage/
    tiff/
  regression/
```

Every discovered bug should become a regression fixture where possible.

### 10.3 Conversion matrix

Test every supported input/output pair, including same-format
conversions.

For each pair, cover relevant options and file/stream modes.

Use semantic image comparisons---not only output hashes.

  Conversion      Comparison
  --------------- ----------------------------------------
  Lossless WebP   Exact pixels
  PNG → PNG       Exact pixels
  JPEG            Tolerance-based
  Lossy WebP      PSNR/SSIM and documented tolerance
  Alpha           Exact/tolerance-based alpha validation
  Animation       Frames, timing, compositing, loop
  Metadata        Field-by-field
  Orientation     Displayed result

### 10.4 External test oracles

Development-time comparisons may use `cwebp`, `dwebp`, `gif2webp`, or
other trusted codec implementations.

They are test tools, not runtime dependencies.

------------------------------------------------------------------------

## 11. Portable Builds and Releases

### 11.1 Targets

-   Windows x64.
-   macOS x86_64.
-   macOS arm64.
-   macOS Universal binary.

### 11.2 macOS

Build both architectures separately, combine with `lipo`, then verify:

``` sh
lipo -create webp-convert-arm64 webp-convert-x64 -output webp-convert
lipo -info webp-convert
otool -L webp-convert
```

Release goals:

-   Universal Mach-O.
-   Self-contained.
-   Signed.
-   Notarized.
-   Stapled.
-   Native testing on Apple Silicon.
-   Native Intel testing or appropriate native runner.

### 11.3 Windows

Target:

``` text
x86_64-pc-windows-msvc
```

Goals:

-   Static runtime where practical.
-   No separately installed codec DLLs.
-   Unicode path support.
-   Binary-safe stdin/stdout.
-   Clean-machine execution.
-   No Rust installation required.

Verify dependencies:

``` text
dumpbin /dependents webp-convert.exe
```

### 11.4 Reproducibility

Pin:

-   Rust toolchain.
-   `Cargo.lock`.
-   Codec versions.
-   Native dependency versions.
-   Build flags.
-   Target triples.
-   Source revision.

### 11.5 Release manifest

Include:

-   CLI version.
-   Commit.
-   Target.
-   Codec versions.
-   Build date.
-   SHA-256.
-   Dependency inventory.
-   License inventory.
-   Capability manifest.
-   Provenance information.

------------------------------------------------------------------------

## 12. 4D Component

### 12.1 Responsibility

The component is a thin wrapper.

It may:

-   Locate the bundled CLI.
-   Verify its integrity/protocol.
-   Normalize options.
-   Construct arguments safely.
-   Launch SystemWorker.
-   Manage timeout and input/output.
-   Parse JSON.
-   Return structured results.

It must not:

-   Search `PATH`.
-   Download a binary at runtime.
-   Invoke a shell.
-   Duplicate codec logic.
-   Parse human-readable output as an API.
-   Silently substitute an executable.

### 12.2 Public API proposal

Finalize namespace and naming before project initialization.

Possible methods:

``` 4d
cs.WebP.Converter.version()
cs.WebP.Converter.capabilities()
cs.WebP.Converter.inspect(input; options)
cs.WebP.Converter.convert(input; output; options)
cs.WebP.Converter.convertBlob(blob; options)
cs.WebP.Converter.convertFile(inputFile; outputFile; options)
```

Use idiomatic 4D component API conventions and the template's structure.

### 12.3 Result model

``` json
{
  "ok": true,
  "exitCode": 0,
  "operation": "convert",
  "input": {},
  "output": {},
  "diagnostics": [],
  "error": null
}
```

Blob conversion should additionally return a Blob result.

Errors should contain structured codes, messages, diagnostics,
operation, exit code, and available input/output details.

### 12.4 File API first

Implement file conversion before Blob conversion.

Benefits:

-   Avoid duplicating large image data in memory.
-   Simplify process integration.
-   Easier debugging.
-   Easier timeout/error behavior.
-   More reliable initial API.

### 12.5 Blob feasibility gate

Prototype SystemWorker binary communication early, but do not let Blob
support block the file-based release.

Verify:

-   Binary stdin.
-   Binary stdout.
-   `dataType` behavior.
-   `postMessage`.
-   Explicit stdin closure.
-   Large Blob behavior.
-   Timeout.
-   Memory use.
-   Byte integrity.
-   Output reconstruction.

If Blob mode is reliable, include it. Otherwise release file mode first
and add Blob later.

### 12.6 Async/future API

Design with future async conversion, progress, cancellation, worker
management, and batch processing in mind. Do not block initial release
on these features.

------------------------------------------------------------------------

## 13. Documentation

### CLI repository

Required:

-   README.
-   CLI reference.
-   Formats/capabilities.
-   Animation semantics.
-   Metadata policy.
-   Color behavior.
-   Streaming.
-   Security/resource limits.
-   Build from source.
-   Testing.
-   Release process.

### Component repository

Required:

-   README.
-   Installation.
-   API reference.
-   Examples.
-   Supported platforms.
-   CLI compatibility.
-   Capabilities.
-   Errors.
-   Blob/file use.
-   Troubleshooting.
-   Version compatibility.
-   Changelog.

Where possible, generate or verify documentation from the capability
registry.

------------------------------------------------------------------------

## 14. CI/CD

### CLI PR pipeline

-   `cargo fmt --check`.
-   Clippy.
-   Unit tests.
-   Codec tests.
-   CLI integration tests.
-   JSON validation.
-   Dependency audit.
-   License audit.
-   Fuzz smoke tests.
-   Target builds where practical.

### Native jobs

-   macOS ARM64.
-   macOS Intel.
-   Windows x64.

Prefer native tests over cross-compilation alone.

### CLI release

-   Build.
-   Test.
-   Verify architectures.
-   Verify dependencies.
-   Generate checksums.
-   Generate manifests.
-   Sign/notarize macOS.
-   Package.
-   Publish.

### Component CI

1.  Validate asset lock.
2.  Verify checksums.
3.  Verify executable architectures.
4.  Verify CLI protocol compatibility.
5.  Compile component with tool4d.
6.  Run component tests.
7.  Package component.
8.  Extract package.
9.  Test extracted package.
10. Publish only after mandatory gates pass.

### Signing

macOS:

-   Developer ID signing.
-   Notarization.
-   Stapling.
-   Verification.

Windows:

-   Document Authenticode status.
-   Verify clean Windows execution.
-   Evaluate signing as a release enhancement.

------------------------------------------------------------------------

## 15. Agent Team and Governance

### Roles

-   **Supervisor/Tech Lead:** Scope, architecture, ADRs, coordination,
    integration, final approval.
-   **CLI/Rust Engineer:** CLI framework, engine, I/O, protocol.
-   **Codec Engineer:** Codecs, animation, metadata, fidelity.
-   **Build/Release Engineer:** Native builds, CI, packaging, signing,
    provenance.
-   **4D Component Engineer:** API, SystemWorker, integration.
-   **QA/Test Engineer:** Corpus, fixtures, matrix, regressions,
    acceptance.
-   **Independent Reviewer:** Security, architecture, fidelity, release
    review.

### Ownership

  Workstream             Owner            Reviewer
  ---------------------- ---------------- ----------------------
  Architecture/ADRs      Supervisor       Independent reviewer
  CLI contract           CLI engineer     Supervisor
  Codec implementation   Codec engineer   QA
  Conversion policy      Codec engineer   Supervisor
  Native build           Build engineer   Independent reviewer
  4D wrapper             4D engineer      Supervisor
  Test corpus            QA               Codec engineer
  Release                Build engineer   Supervisor
  Acceptance             QA               Independent reviewer

### Agent rules

All agents must:

1.  Read `AGENTS.md`.
2.  Read approved contracts/ADRs.
3.  Confirm scope before implementation.
4.  Respect file ownership.
5.  Avoid independently modifying shared contracts.
6.  Avoid unapproved dependencies.
7.  Document license implications.
8.  Add tests for new functionality.
9.  Report actual test results.
10. Identify platforms not tested.
11. Never claim unverified success.
12. Record deviations.
13. Use small, reviewable changes.
14. Keep unsupported behavior explicit.
15. Provide a structured completion report.

### Completion report template

``` text
Task:
Files changed:
Design/contract followed:
Implementation summary:
Dependencies/licenses added:
Tests run:
Test results:
Platforms tested:
Platforms not tested:
Known limitations:
Potential regressions:
Commit:
Ready for review:
```

------------------------------------------------------------------------

## 16. Roadmap and Gates

Time estimates are indicative. Actual progress depends on codec
feasibility, native builds, testing, and platform availability.

### Phase 0 --- Scope and decisions (2--4 days)

Tasks:

-   Decide names/repositories.
-   Confirm Rust and WebP backend candidate.
-   Audit component template and conventions.
-   Define format matrix.
-   Define v1 scope.
-   Define animation, metadata, alpha, color, orientation, and TIFF
    policies.
-   Define resource-limit approach.
-   Define CLI/protocol/exit codes.
-   Define 4D baseline version.
-   Define fixture requirements.
-   Create ADRs.
-   Create risk register.

Deliverables: requirements, architecture draft, CLI contract, matrix,
codec research, license audit, ADRs.

**Gate G0:** No unresolved decision that materially affects
implementation.

### Phase 1 --- Codec/portability proof (3--7 days)

Prototype codecs, compare backends, verify animation/metadata/alpha,
benchmark quality/speed/size, build Windows and macOS targets, inspect
dependencies/licenses.

Deliverable: feasibility report with evidence.

**Gate G1:** Required codecs and portable builds demonstrated.

### Phase 2 --- CLI foundation (3--5 days)

-   Initialize repo.
-   Pin Rust.
-   Implement CLI parser.
-   Core types.
-   Input/output resolver.
-   Format detection.
-   Error taxonomy.
-   Diagnostics.
-   JSON protocol.
-   Capability registry.
-   Version/formats/capabilities/inspect commands.

**Gate:** Contract tests pass.

### Phase 3 --- Static conversion (1--2 weeks)

-   WebP lossy/lossless.
-   PNG, JPEG, GIF, BMP.
-   TIFF, or documented exclusion.
-   Alpha.
-   Background flattening.
-   Quality mappings.
-   Metadata/orientation baseline.
-   Conversion validation.
-   Pixel tests.

**Gate:** Supported static conversion matrix passes.

### Phase 4 --- Streams and safety (4--7 days)

-   All file/stdin/stdout combinations.
-   Binary-safe Windows.
-   Buffering.
-   Output contamination tests.
-   Resource limits.
-   Timeout.
-   Atomic output.
-   Overwrite behavior.
-   Alias detection.
-   Cleanup.
-   Corrupt-input tests.

**Gate:** CLI independently usable.

### Phase 5 --- Advanced semantics (1--2 weeks, scope-dependent)

-   Metadata policies.
-   ICC/EXIF/XMP.
-   Orientation.
-   16-bit/CMYK behavior.
-   Animation detection and frame handling.
-   GIF/WebP animation.
-   Timing/disposal/loop.
-   TIFF pages.
-   Diagnostics.

**Gate:** No silent loss of significant data.

### Phase 6 --- Hardening and CLI release (about 1 week)

-   Fuzz.
-   Audits.
-   Security review.
-   Performance.
-   Large images.
-   Regression tests.
-   Binary size.
-   Architecture/dependency verification.
-   Clean-machine tests.
-   Documentation.
-   Release notes.
-   Manifests/checksums/provenance.

**Gate:** CLI release candidate passes acceptance.

### Phase 7 --- Component foundation (3--5 days)

-   Initialize from template.
-   API/result/error model.
-   Resolver.
-   Asset lock/checksum.
-   Protocol compatibility.
-   Version/capabilities/inspect.
-   File conversion.
-   Timeout.
-   Diagnostics.

**Gate:** 4D invokes bundled CLI without PATH, shell, or external
install.

### Phase 8 --- Blob feasibility/API (3--7 days)

-   Prototype binary SystemWorker input/output.
-   Verify binary data type and message behavior.
-   Explicit stdin close.
-   Large Blob.
-   Timeout.
-   Memory.
-   Integrity.
-   Limits.
-   Documentation.

Release Blob if reliable; otherwise retain file-based release fallback.

### Phase 9 --- Component integration (4--7 days)

-   Native platform tests.
-   tool4d compilation.
-   Component tests.
-   File conversion.
-   Blob if implemented.
-   Error/timeout.
-   Missing executable.
-   Hash and architecture.
-   Package/unpack/test.

**Gate:** Final packaged component passes integration tests.

### Phase 10 --- Release (3--5 days)

-   Freeze API/protocol.
-   Verify assets.
-   Verify hashes.
-   Signing/notarization.
-   License audit.
-   Clean-machine tests.
-   Documentation.
-   Changelog.
-   Known limitations.
-   Release.
-   Acceptance evidence.

**Gate:** Release approved.

### Phase 11 --- Post-v1

Batch, recursive conversion, additional formats, multi-page TIFF,
resizing/cropping, color conversion, metadata editing, progress,
cancellation, async API, parallel processing.

------------------------------------------------------------------------

## 17. Work Breakdown Structure (GitHub Issues)

  ID      Work package             Dependency     Deliverable
  ------- ------------------------ -------------- ------------------------
  WP-01   Scope and requirements   None           Approved requirements
  WP-02   Codec research           WP-01          Codec comparison
  WP-03   License audit            WP-02          License inventory
  WP-04   WebP prototype           WP-02          Prototype results
  WP-05   Portability proof        WP-04          Native target binaries
  WP-06   Architecture/ADRs        WP-01--05      Approved architecture
  WP-07   CLI contract             WP-06          Frozen protocol
  WP-08   CLI skeleton             WP-07          Working CLI
  WP-09   Static conversion        WP-08          Core conversions
  WP-10   Stream mode              WP-08          Binary-safe pipelines
  WP-11   Capability registry      WP-08          Capability model
  WP-12   Alpha/color              WP-09          Conversion policies
  WP-13   Metadata/orientation     WP-09          Metadata behavior
  WP-14   Animation                WP-06, WP-09   Animation
  WP-15   Resource guards          WP-08, WP-09   Limits
  WP-16   Test corpus              Parallel       Fixtures
  WP-17   CLI CI                   WP-08          CI
  WP-18   CLI hardening            WP-09--17      RC
  WP-19   CLI release              WP-18          Portable binaries
  WP-20   Component init           WP-06          4D project
  WP-21   Asset pinning            WP-19, WP-20   Locked assets
  WP-22   4D file API              WP-21          File conversion
  WP-23   Blob prototype           WP-06          Feasibility
  WP-24   Blob API                 WP-23          Blob conversion
  WP-25   Component tests          WP-22, WP-24   Acceptance tests
  WP-26   Component CI             WP-20--25      Packaging pipeline
  WP-27   Documentation            All            Documentation
  WP-28   Security review          WP-18, WP-25   Security report
  WP-29   Final acceptance         All            Acceptance checklist
  WP-30   Component release        WP-29          Public release

### Parallel work

Early parallel work may include codec research, corpus creation, CLI
contract design, licensing, native builds, Blob feasibility, and
template research.

Do not start overlapping production implementation until interfaces and
ownership are approved.

------------------------------------------------------------------------

## 18. ADR Register

Create and maintain ADRs for at least:

-   ADR-001: Language and workspace structure.
-   ADR-002: WebP codec/backend.
-   ADR-003: Format scope and priorities.
-   ADR-004: Internal image/frame model.
-   ADR-005: CLI syntax and protocol.
-   ADR-006: Binary stdout/stderr separation.
-   ADR-007: Alpha and flattening policy.
-   ADR-008: Metadata policy.
-   ADR-009: Orientation/color handling.
-   ADR-010: Animation semantics and release scope.
-   ADR-011: Resource limits/security.
-   ADR-012: Repository strategy.
-   ADR-013: 4D executable packaging and asset pinning.
-   ADR-014: Blob integration.
-   ADR-015: Native build and release process.

Each ADR must contain context, decision, alternatives, consequences,
validation evidence, and status.

------------------------------------------------------------------------

## 19. Risk Register

  Risk                           Impact        Mitigation
  ------------------------------ ------------- --------------------------------
  Codec capability gaps          High          Prototype early
  Animation backend incomplete   High          Separate milestone
  Static linking failures        High          Native portability proof
  Backend quality differences    Medium        Benchmark and fidelity tests
  Metadata loss                  High          Policy and diagnostics
  Alpha loss                     High          Explicit flattening
  TIFF pages lost                High          Reject or page-aware support
  Decompression bombs            High          Resource limits
  Binary stdout corrupted        High          Native stream tests
  Blob incompatibility           Medium/High   Early prototype; file fallback
  Protocol drift                 High          Versioning/compatibility tests
  Platform test gaps             High          Native jobs required
  License conflict               High          Audit before adoption
  Nonportable releases           High          Clean-machine acceptance
  Scope creep                    High          Gates and backlog
  Agent conflicts                Medium        Ownership/supervisor
  Wrong bundled binary           High          Hash and asset lock
  Breaking API                   Medium/High   Freeze before release

Actively monitor:

1.  Codec capability and animation parity.
2.  Native binary portability.
3.  4D Blob communication.
4.  Image fidelity/data-loss behavior.
5.  Dependency licensing/provenance.

------------------------------------------------------------------------

## 20. Acceptance Criteria

### CLI

-   [ ] Version works.
-   [ ] Capabilities report actual support.
-   [ ] Supported conversion matrix passes.
-   [ ] File-to-file works.
-   [ ] File-to-stdout works.
-   [ ] Stdin-to-file works.
-   [ ] Stdin-to-stdout works.
-   [ ] Binary stdout clean.
-   [ ] Exit codes stable.
-   [ ] JSON validated.
-   [ ] Format detection works.
-   [ ] Alpha behavior correct.
-   [ ] Animation behavior explicit.
-   [ ] Metadata tested.
-   [ ] Corrupt inputs safe.
-   [ ] Limits enforced.
-   [ ] Overwrite behavior correct.
-   [ ] Atomic output where supported.
-   [ ] Fuzz tests run.
-   [ ] Regression tests pass.
-   [ ] Performance recorded.

### Platforms

-   [ ] Windows x64 native build/test.
-   [ ] Windows dependency inspection.
-   [ ] Unicode paths.
-   [ ] macOS Intel native build/test.
-   [ ] macOS ARM native build/test.
-   [ ] Universal binary verified.
-   [ ] macOS dependency inspection.
-   [ ] Clean-machine test.
-   [ ] Checksums.
-   [ ] Signing.
-   [ ] Notarization.
-   [ ] Release manifests.

### Component

-   [ ] Compiles.
-   [ ] API/namespace verified.
-   [ ] Resolver works.
-   [ ] Bundled CLI verified.
-   [ ] Hash verified.
-   [ ] No PATH dependency.
-   [ ] No shell invocation.
-   [ ] File conversion.
-   [ ] Inspect.
-   [ ] Capabilities.
-   [ ] Errors.
-   [ ] Timeout.
-   [ ] Protocol compatibility.
-   [ ] Blob if released.
-   [ ] Package unpacked and tested.
-   [ ] Clean environment.

### Release

-   [ ] Dependency audit.
-   [ ] License audit.
-   [ ] Security review.
-   [ ] Documentation.
-   [ ] Known limitations.
-   [ ] Release notes.
-   [ ] Checksums/manifests.
-   [ ] CLI tag.
-   [ ] Component tag.
-   [ ] Acceptance evidence.
-   [ ] Supervisor approval.

------------------------------------------------------------------------

## 21. Recommended Repository Layout

Adapt to the actual template and established Rust conventions.

### CLI

``` text
webp-convert-cli/
├── AGENTS.md
├── README.md
├── LICENSE
├── SECURITY.md
├── CHANGELOG.md
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── crates/
│   ├── core/
│   │   ├── image_model/
│   │   ├── conversion_policy/
│   │   ├── capabilities/
│   │   └── diagnostics/
│   ├── codec-webp/
│   ├── codec-png/
│   ├── codec-jpeg/
│    ├── codec-gif/
│   ├── codec-tiff/
│   ├── codec-bmp/
│   └── cli/
│       ├── arguments/
│       ├── commands/
│       ├── input/
│       ├── output/
│       └── protocol/
├── docs/
│   ├── architecture.md
│   ├── cli.md
│   ├── formats.md
│   ├── animation.md
│   ├── metadata.md
│   ├── color.md
│   ├── streaming.md
│   ├── security.md
│   ├── testing.md
│   ├── release.md
│   └── adr/
├── tests/
│   ├── fixtures/
│   ├── unit/
│   ├── codec/
│   ├── integration/
│   ├── protocol/
│   ├── regression/
│   └── fuzz/
├── scripts/
├── tooling/
└── .github/workflows/
```

### 4D component

``` text
webp-convert/
├── AGENTS.md
├── README.md
├── package.json
├── Documentation/
├── Project/
│   └── Sources/
│       └── Classes/
│           ├── _CLI.4dm
│           ├── _Controller.4dm
│           └── Converter.4dm
├── Resources/
│   ├── bin/
│   │   ├── macOS/webp-convert
│   │   └── Windows/webp-convert.exe
│   ├── licenses/
│   └── manifests/
├── cli/
│   └── asset-lock.json
├── tests/
├── scripts/
└── .github/workflows/
```

Do not force this layout if the template or established conventions
require another structure.

------------------------------------------------------------------------

## 22. Immediate Kickoff --- First 10 Issues

Create these GitHub issues first:

1.  **T01 --- Confirm project scope and v1 format matrix**
2.  **T02 --- Audit current 4D component template and `AGENTS.md`**
3.  **T03 --- Audit quickdraw CLI/component release architecture**
4.  **T04 --- Evaluate Rust WebP codec candidates**
5.  **T05 --- Verify animation capabilities and semantics**
6.  **T06 --- Audit dependency licenses**
7.  **T07 --- Prototype Windows x64 portable build**
8.  **T08 --- Prototype macOS Universal build**
9.  **T09 --- Prototype 4D SystemWorker Blob communication**
10. **T10 --- Finalize architecture and CLI protocol ADR**

Do not launch the full implementation before these tasks produce enough
evidence to approve the architecture.

------------------------------------------------------------------------

## 23. Final Execution Order

1.  Prove codec technology, licensing, portability, and streaming.
2.  Define CLI syntax, protocol, exit codes, and image semantics.
3.  Build the standalone CLI.
4.  Test all supported conversions and stream combinations.
5.  Harden the CLI.
6.  Release verified Windows x64 and macOS Universal binaries.
7.  Pin those assets in the 4D component.
8.  Implement the 4D file API.
9.  Add Blob integration if proven reliable.
10. Package, test the extracted package, and publish.

### Five non-negotiable principles

-   The CLI must be independently useful.
-   Significant image data must never be silently lost.
-   Portability must be demonstrated, not assumed.
-   The 4D component must remain thin.
-   Agents must work against stable, approved contracts.

------------------------------------------------------------------------

## 24. Final Project Definition

The project is a portable, reliable, security-conscious image conversion
CLI supporting WebP and a practical set of common formats, binary-safe
file and stream operation, explicit image semantics, structured
diagnostics, reproducible releases, and a thin 4D component wrapper.

The implementation strategy is intentionally incremental:

> **Prove the technology → freeze contracts → build the CLI → harden and
> release it → integrate into 4D → verify and release the component.**

The first deliverable is not a large codebase. It is a successful
feasibility sprint with documented evidence and approved architecture.
Once that gate is passed, implementation can proceed with significantly
lower risk and clear ownership.
