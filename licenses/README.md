# Third-party license files

This directory holds the license texts that must ship with every binary
release of `webp-convert` (see `docs/dependency-licenses.md`,
"Redistribution duties").

## libwebp/ — vendored native libwebp 1.5.0

Bundled into every binary via [`libwebp-sys2`
0.2](https://crates.io/crates/libwebp-sys2) (`c_src/`, builtin-vendored
build). libwebp is BSD-3-Clause (Google Inc.); the binding crate itself is
BSD-3-Clause (Masaki Hara).

| File | Source | Why it ships |
| --- | --- | --- |
| `COPYING` | libwebp `COPYING` | BSD-3-Clause notice (mandatory in binary distributions) |
| `AUTHORS` | libwebp `AUTHORS` | Contributor list referenced by the notice |
| `PATENTS` | libwebp `PATENTS` | Google's additional IP rights grant |
| `LICENSE-libwebp-sys2.txt` | libwebp-sys2 `LICENSE` | Binding-crate notice for the same binary |

## Refreshing

After bumping `libwebp-sys2` in `Cargo.toml`, re-run:

```sh
scripts/refresh-licenses.sh
```

It locates the vendored sources in the local cargo registry, verifies the
libwebp version recorded in the vendored `configure.ac`, and re-copies the
files above. A checksum of this directory is recorded in every release
manifest (`scripts/release-manifest.sh`).
