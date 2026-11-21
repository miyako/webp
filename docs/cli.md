# webp-convert — CLI reference (v0)

Status: implemented, building and tested on macOS aarch64 (Rust 1.100.0). CI
covers macOS arm64/x64 and Windows x64 native builds. Everything below
describes what the binary actually does — for format-level limitations also
run `webp-convert capabilities`, which reports the *linked* codecs, never
aspirational ones.

## Synopsis

```text
webp-convert [OPTIONS] INPUT OUTPUT        # bare form (the default: convert)
webp-convert convert INPUT OUTPUT          # same, explicit subcommand
webp-convert inspect INPUT
webp-convert formats
webp-convert capabilities [FMT]
```

INPUT may be `-` (stdin), OUTPUT may be `-` (stdout). Both forms share the
same option namespace.

## Exit codes (stable, plan §7.5)

| code | meaning |
| --- | --- |
| 0 | success |
| 2 | invalid arguments / missing mandatory option (e.g. JPEG target without `--background`) |
| 3 | corrupt, truncated, unreadable or unsupported input; animated input without `--frame` |
| 4 | resource limit exceeded (`--max-pixels-mp`, `--max-input-bytes`) |
| 5 | output error (exists without `--overwrite`, unwritable, alias of input) |
| 6 | internal error (bugs; never policy) |
| 7 | protocol error |

Usage errors from the argument parser are also exit 2 (`--help`/`--version`
exit 0 via stdout).

## No silent data loss (the core contract)

The engine refuses conversions that would drop information without an
explicit instruction:

- **Animated input** (animated WebP/GIF with >1 frame) to a still target is
  refused (exit 3) unless `--frame N` picks one or `--flatten` explicitly
  flattens to the first frame (with an `animation_flattened` warning).
  Animated *output* to WebP or GIF is supported when no selection flags are
  given; still targets remain refused rather than dropping frames.
- **Alpha into an alpha-less target** (JPEG, BMP) requires `--background
  #RRGGBB` (exit 2). With it, pixels are composited and an
  `alpha_flattened` diagnostic is emitted.
- **Multi-page TIFF** requires `--page N` (refused otherwise — never
  silently writing page 0). `--page` and `--frame` cannot be combined.
- **Metadata** (v0): input ICC/EXIF/XMP/comments are detected (presence
  probe) and a `metadata_dropped` warning is emitted because no target
  codec writes metadata through yet. `--metadata none` strips explicitly
  with an info diagnostic.
- **Overwrites** require `--overwrite`; an output path that resolves to the
  input file is always refused. Writes are atomic (temp file + rename).
- **Lossless honesty**: `--lossless`/`--near-lossless` are WebP-only. The
  WebP codec sets `exact=1` and imports via ARGB in lossless mode, so RGB
  under fully-transparent pixels is preserved bit-exactly (verified by
  round-trip tests).

## Options

| option | meaning |
| --- | --- |
| `-i, --input FILE` / `-o, --output FILE` | explicit I/O (stdin `-`, stdout `-`) |
| `--from FMT` | force input format (usually sniffing suffices) |
| `--format FMT` / `--to FMT` | force output format, overriding the extension |
| `-q, --quality 0-100` | codec-defined quality (WebP/JPEG; not comparable between them) |
| `--lossless` | WebP lossless (pixel-exact, `exact=1`) |
| `--lossy` | WebP lossy (explicit default; conflicts with `--lossless`/`--near-lossless`) |
| `--method 0-6` | encoder effort (WebP; higher = slower/smaller) |
| `--alpha-quality 0-100` | WebP alpha plane quality |
| `--near-lossless 0-100` | WebP near-lossless strength (implies lossless) |
| `--background #RRGGBB` | flattening color; mandatory for alpha loss |
| `--metadata all\|icc\|none` | metadata intent (v0: all/icc diagnose drops, none strips) |
| `--orient auto\|none` | apply EXIF orientation to pixels vs keep as stored (default none, warned) |
| `--frame N` | decode frame N (0-based) of an animated input |
| `--page N` | decode page N of a multi-page input (0-based; TIFF) |
| `--flatten` | flatten an animated input to its first frame (warned) |
| `--max-pixels-mp MP` | pixel budget (default 100 Mp) |
| `--max-input-bytes BYTES` | input size budget (default 512 MiB) |
| `--max-frames N` | frame-count budget, checked from the container header *before* decode (default: unlimited; animated input is refused without `--frame` anyway) |
| `--timeout SECONDS` | processing deadline, checked between pipeline stages (read → decode → encode → write); breach exits 4. Not a hard kill: blocking native codec calls and a stalled stdin cannot be interrupted from inside the process — enforce a hard limit in the wrapper if that matters |
| `--overwrite` | allow replacing an existing output file |
| `--dry-run` | run detect/decode/policy, write nothing, report intended output |
| `--json` | machine-readable protocol-v1 report |
| `--quiet` | suppress informational stderr lines |

## JSON protocol (v1)

`--json` turns every result into a single document:

```json
{
  "protocol": 1,
  "ok": true,
  "operation": "convert",
  "input":   { "format": "png", "width": 4, "height": 4, "frames": 1, "has_alpha": true },
  "output":  { "format": "webp", "mode": "/tmp/out.webp", "bytes": 132 },
  "diagnostics": [ { "code": "alpha_flattened", "severity": "info", "message": "..." } ],
  "error": null
}
```

On failure `ok` is false, `error` carries `{"code", "message"}` with the
stable codes from plan §7.4 (`missing_option`, `unsupported_input`,
`output_error`, …), and the exit code matches the table above.

**stdout discipline (ADR-006):** when the *image* goes to stdout (`-o -`),
reports never join it — the JSON/human report moves to stderr. Otherwise
JSON reports go to stdout. Human-readable progress/diagnostics always go to
stderr, so stdout stays scriptable either way.

## Streams

All four input/output combinations work in both call forms:

| form | input | output |
| --- | --- | --- |
| `webp-convert in.png out.webp` / `convert in.png out.webp` | file | file |
| `webp-convert - out.webp` / `cat \| webp-convert - out.webp` | stdin | file |
| `webp-convert in.png -` | file | stdout |
| `webp-convert - -` / `cat in.png \| webp-convert - -` | stdin | stdout |

Rules:

- Input format is detected from magic bytes regardless of stream (no
  extension to lean on for stdin); `--from FMT` forces it.
- **`-` as output plus no `--format` keeps the input format.** Every other
  output name takes its format from its extension (overridable).
- Emptiness and limits on stdin: an empty stdin is exit 3; stdin over
  `--max-input-bytes` is exit 4 (the reader stops *after* the budget, it
  never silently truncates at it).
- A run that fails at any stage writes nothing to stdout and produces no
  partial file; the report (JSON or human) goes to stderr in `-o -` mode.

## Reporting what a build can do

```text
webp-convert formats            # one line: supported formats
webp-convert capabilities             # human-readable caps + restrictions
webp-convert capabilities --json      # protocol-v1 document, "capabilities" array
```

Entries come from the linked codec code (format, read/write, animation,
alpha, metadata categories, pages, bit-depth limits, backend + version,
restrictions). Docs never claim more than this command reports.

## Examples

```bash
webp-convert photo.png photo.webp                 # plain convert
webp-convert photo.png photo.webp --lossless      # pixel-exact (exact=1)
webp-convert photo.png shot.jpg --background '#ffffff'
webp-convert anim.webp frame.png --frame 0        # explicit frame pick
webp-convert scan.jpg scan.png --orient auto      # apply EXIF rotation
cat img.heic | webp-convert - out.png             # (heic unsupported -> exit 3)
webp-convert photo.png - --to webp --json | jq .output.bytes  # to stdout, JSON on stderr
```

## Known limitations in v0 (honest list)

- No metadata write-through to any format (ICC/EXIF/XMP chunks are not
  copied; presence is diagnosed). P1.
- APNG input decodes to its first frame (documented, diagnosed in caps).
- 16-bit PNG/TIFF normalize to 8-bit with a diagnostic.
- CMYK TIFF pages convert with a naive 1-minus mapping (no ICC profile math).
- Animated input to a still target requires explicit selection
  (`--frame N` or `--flatten`); animation output is webp/gif only.
- GIF animation timing quantizes to 1/100 s (inherent to the format).
