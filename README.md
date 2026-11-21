# webp

Convert to and from the WebP image format.

`webp-convert` is a portable, loud-by-default image conversion CLI:
6 formats in (WebP, PNG, JPEG, GIF, TIFF, BMP), 6 out, with honest
diagnostics instead of silent losses, a versioned JSON report protocol
(`--json`), hard resource limits (`--max-pixels-mp`, `--max-frames`,
`--max-input-bytes`, `--deadline`), and a self-contained ~3 MiB binary.

```text
webp-convert -i photo.png -o photo.webp            # convert a file
webp-convert -i - -o - --to webp --json | jq .     # stdin → stdout
webp-convert -i anim.webp -o frame.png --frame 2   # pick a frame
webp-convert -i multi.tif -o p0.png --page 0       # pick a TIFF page
webp-convert -i a.jpg -o b.webp --orient auto      # rotate per EXIF
```

Key behaviors (details in [docs/cli.md](docs/cli.md)):

- **No silent data loss** — pages, frames, alpha, bit depth, and metadata
  are either preserved, explicitly selected, or loudly diagnosed; every
  conversion carries a machine-readable diagnostic list.
- **Metadata** — ICC/EXIF/XMP are written through to WebP targets
  (`--metadata icc|all|none`); other targets drop them with warnings.
- **Atomic writes** — temp file + rename; `--overwrite` is required;
  writing over the input is always refused.

## 4D component

This repository also releases a compiled 4D component (namespace
**WebP**, tags without `v`, e.g. `20.0.0`, asset `webp.zip`) that wraps
the pinned CLI executable — the CLI ships preinstalled and is
SHA-256-verified at every call.

```4d
var $converter : cs.WebP.Converter
$converter:=cs.WebP.Converter.new("macOS-universal")  // or no argument on macOS
$result:=$converter.convert(File("/DATA/photo.png"); File("/DATA/photo.webp"); New object("quality"; 82))
If ($result.ok)
  // $result.value.output.bytes, plus machine-readable $result.value.diagnostics
Else
  ALERT($result.failureKind+": "+$result.error.message)
End if
```

Install via the 4D dependency manager (`miyako/webp`, release `20.0.0`)
or unzip `webp.zip` into your project's `Components` folder. Full user
guide — installation, API, examples, platforms, errors, troubleshooting:

- [docs/component.md](docs/component.md) — using the 4D component

## Build

```sh
cargo build --release          # ~35 s cold, one 3.2 MiB binary
cargo test --release           # 94 tests
```

Requires a stable Rust toolchain (see `rust-toolchain.toml`); no system
dependencies. Cross-target WebP decoding/encoding uses libwebp via
`libwebp-sys`, vendored through crates.io.

## Documentation

- [docs/cli.md](docs/cli.md) — full CLI reference, diagnostics, limits,
  and the known-limitations list
- [docs/component.md](docs/component.md) — 4D component user guide
  (installation, API, platforms, errors, compatibility)
- [docs/release-notes-v1.0.0.md](docs/release-notes-v1.0.0.md) — what's
  in the current release
- [docs/adr/ADRs.md](docs/adr/ADRs.md) — design decisions
- [docs/phase-6-report.md](docs/phase-6-report.md) — hardening, fuzzing,
  audit, and performance evidence

## License

MIT — see [LICENSE](LICENSE). Dependency license summary in
[docs/dependency-licenses.md](docs/dependency-licenses.md).
