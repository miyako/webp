# Using the webp 4D component

This guide is for 4D developers who consume the compiled component
(release asset `webp.zip`, tag `20.0.0` and later). It follows the
master-plan §Component-repository checklist: installation, API reference,
examples, supported platforms, CLI compatibility, capabilities, errors,
blob/file use, troubleshooting, version compatibility, changelog.
Maintainer-facing build/release documentation lives in
[4d/README.md](4d/README.md).

The component is **not a plug-in**: it starts a bundled standalone Rust
executable (`webp-convert`, preinstalled under the component's
`Resources/bin/`), verified by SHA-256 at every invocation — no PATH
search, no shell, no runtime downloads.

## Installation

Requirements: 4D v20.7 or later (the shipped compiled binary is built
with the pinned 4D 20.7 tool4d build), macOS on Apple Silicon or any
Intel Mac (the bundled CLI is a Universal binary).

In 4D (≥ v20):

1. Open your project → **Settings/Project → Dependencies** (or the
   Dependencies tab).
2. Add a dependency: **GitHub**, repository `miyako/webp`.
3. Pin the version: select release/tag `20.0.0` (component releases are
   prefix-free SemVer, independent of the CLI's `v`-prefixed tags).
4. 4D fetches the release asset `webp.zip`, installs the component and
   offers to restart the project. The namespace **WebP** becomes
   available (`cs.WebP`).

Manual alternative for environments without dependency-manager access:
download `webp.zip` from the release and unzip it into your project's
`Components` folder (the archive's top level is `webp.4dbase/`).

### Gatekeeper (single first launch)

The bundled executable is currently not Developer-ID notarized; macOS
Gatekeeper may ask for confirmation the first time a fresh component
copy launches it. Right-click → Open once, or run
`xattr -dr com.apple.quarantine webp.4dbase` after downloading with a
browser (quarantine is applied only to browser downloads; programmatic
fetches and dependency-manager installs typically don't carry it).
A future component release may carry Apple notarization — versions with
signing are listed in `4d/release-policy.json`.

## Quickstart

```4d
var $converter : cs.WebP.Converter
var $target : Text
$target:="macOS-universal"   // or omit the argument on macOS
If (Is Windows)
  $target:="Windows-x64"     // fails on 20.0.0; see Supported platforms
End if
$converter:=cs.WebP.Converter.new($target)

var $result : Object
$result:=$converter.convert(File("/DATA/photo.png"); File("/DATA/photo.webp"); New object("quality"; 82))
If ($result.ok)
  ALERT("Wrote "+String($result.value.output.bytes)+" bytes"
    +", diagnostics: "+JSON Stringify($result.value.diagnostics))
Else
  ALERT($result.failureKind+": "+$result.error.code+" — "+$result.error.message)
End if
```

Loud by default: successful conversions still return every diagnostic
in `value.diagnostics` — preserve them (and the input file) in your
workflow; a diagnostic like `animated_dropped` may be exactly the data
loss you need to see.

## API reference

`cs.WebP.Converter` — full reference with the complete failure-kind
table is available in the 4D method explorer (open the component's
`Converter` class) and in the repository at
[`4d/component/webp/Documentation/Classes/Converter.md`](4d/component/webp/Documentation/Classes/Converter.md).

| Method | Signature | Returns |
| --- | --- | --- |
| `version()` | no args | `{version:"1.0.0"}` parsed from the pinned CLI and cross-checked against the asset metadata |
| `capabilities()` | no args | the CLI capability document (formats, metadata support, limits) |
| `inspect(input)` | `input`: Text path or `4D.File` | structure probe: `{format,width,height,frames,has_alpha}` — dimensions not pixels |
| `convert(input; output; options)` | paths/File objects; options object | `{input:{...},output:{format,mode,bytes}}` plus diagnostics |

Every method returns **one envelope**:

- success → `{ok:true; value:<document>}`
- failure → `{ok:false; failureKind:<kind>; error:{code; message}}`

`failureKind`: `argument` (rejected before launch), `process` (cannot
resolve/launch), `asset` (verified-asset metadata mismatch), `converter`
(the CLI ran and reported e.g. `corrupt_input`,
`resource_limit_exceeded`), `protocol` (output violated the pinned wire
contract; impossible on a verified release — treat as a bug report).

### Options (convert)

Booleans: `lossless, lossy, flatten, overwrite, dry-run (dryRun is not
accepted; use "dry-run")`. Numbers: `quality, method, alpha-quality,
near-lossless, frame, page, max-pixels-mp, max-input-bytes, max-frames,
timeout`. Texts: `format, to, from, background, metadata, orient`.
Unknown options are rejected with `failureKind:argument` before the
process starts — there is deliberately no passthrough.

Important details:

- **Path semantics** — use `4D.File` objects or absolute paths. Relative
  Text paths resolve inside the component bundle, not your data folder.
- **No clobber** — a failing conversion cannot prove the output file was
  not written; the CLI commits atomically, but never auto-retry.
- **Timeout** — the process wrapper enforces a hard 30 s per-invocation
  timeout (then `process_terminated`); the `timeout` option is the
  CLI-internal processing deadline, checked between pipeline stages.

## Examples

Inspect before converting (recommended gate):

```4d
var $insp : Object
$insp:=$converter.inspect(File("/DATA/scan.tiff"))
If ($insp.ok && $insp.value.frames>1)
  // TIFF multi-page: select a page explicitly
  $result:=$converter.convert(File("/DATA/scan.tiff"); File("/DATA/p0.webp"); New object("page"; 0))
End if
```

Losing no metadata:

```4d
$result:=$converter.convert(File("/DATA/in.jpg"); File("/DATA/out.webp"); New object("metadata"; "all"; "orient"; "auto"))
// metadata:"icc" writes ICC only; non-WebP targets cannot carry WebP
// metadata chunks and warn with metadata_dropped instead.
```

Batch with per-image limits and a deadline (CLI-internal):

```4d
var $file : 4D.File
var $files : Collection
var $opts : Object
$opts:=New object("quality"; 75; "max-pixels-mp"; 20; "max-input-bytes"; 50000000; "timeout"; 10)
$files:=New collection(File("/DATA/a.jpg"); File("/DATA/b.png"))
For each ($file; $files)
  $status:=$converter.convert($file; File($file.parentFolder.platformPath+$file.stem+".webp"); $opts)
  If (Not($status.ok))
    ALERT($file.name+": "+$status.error.code)
  Else
    ASSERT($status.value.diagnostics.length>=0)  // keep or log diagnostics
  End if
End for each
```

## Supported platforms (current release)

| Platform | State |
| --- | --- |
| macOS arm64 / Intel (Universal CLI) | ✅ released and host-accepted |
| Windows x64 | ❌ component 20.0.0 hard-fails on construction (`failureKind:"asset"`) — no verified Windows CLI asset exists in the lock yet; the component refuses rather than running unverified code, per ADR-015 |
| Linux | not applicable (no 4D desktop server runtime target for this component) |

## Capabilities and CLI compatibility

Capabilities are read from the pinned CLI at runtime
(`capabilities()`); do not hard-code the format matrix. The component
pins CLI **webp-convert v1.0.0** through `cli/asset-lock.json` — exact
source commit and SHA-256 for ZIP, manifest and executable. Component
minor releases (e.g. 20.1.0) may pin a newer CLI; the CLI's own version
ladder (`v1.x`) is visible in `version()`.

## File mode vs Blob mode

Component 20.0.0 is **file/paths only** — no stdin-image, no Blob API.
ADR-014 defers Blob integration behind a SystemWorker feasibility gate
(binary stdin/stdout, timeouts, memory ceilings, byte integrity). File
mode means: you own the input and output files; nothing leaves disk.

## Troubleshooting

| Symptom | Meaning / action |
| --- | --- |
| `failureKind:"asset"`, `asset_mismatch` | The bundled binary or its manifests don't match the lock. Reinstall the component release; do not repair by hand. |
| `missing_executable` / `unsupported_architecture` | Broken install or wrong platform: on Windows a 20.0.0 component is expected to refuse. Reinstall; check the release notes. |
| `process_terminated` | The 30 s wrapper timeout fired. Convert smaller inputs, split batch jobs, raise nothing silently. |
| `resource_limit_exceeded` | Raise `max-pixels-mp` / `max-input-bytes` / `max-frames` deliberately, or reject the file first. |
| Gatekeeper prompt on first use | See installation section. |
| Anything with `failureKind:"protocol"` | Wire-contract violation — file a bug with `webp.acceptance.json` from the release and the CLI's version. |
| `metadata_dropped` diagnostics | Non-WebP target with metadata present — expected, loud behavior, not a bug. |

## Version compatibility policy

- Component version (this release: **20.0.0**) is independent from the
  CLI version (**1.0.0**). A component release always pins an exact CLI
  release; it never floats.
- Component tags carry no `v` prefix; CLI tags always do. Matching
  release assets: component → `webp.zip`; CLI →
  `webp-convert-<version>-<target>.zip`.
- Protocol version (`protocol:1`) is stable within a major CLI version.

## Changelog

- **20.0.0** (2026-11-21) — first release. Converter/CLI/Process
  classes, pinned webp-convert 1.0.0 (macOS Universal), digest-verified
  assets, hosted compiled-host acceptance, unsigned distribution.
