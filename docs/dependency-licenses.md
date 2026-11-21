# Dependency & License Inventory

Generated from the feasibility prototype tree (`tooling/codec-prototype/`,
2026-11-21, Rust 1.100.0). CI (WP-03/WP-17) must regenerate this at every
release. All licenses are permissive; none conflict with this repository's
MIT license.

## License summary (52 crates)

| License | Count | Crates |
| --- | --- | --- |
| Apache-2.0 OR MIT | 29 | image, webp, webp-animation, gif, png, libc, flate2, syn, quote, proc-macro2, … |
| MIT | 6 | tiff, libwebp-sys, color_quant, crunchy, fax, simd-adler32 |
| Apache-2.0 OR MIT OR Zlib | 5 | bytemuck, miniz_oxide, zune-core, zune-jpeg, … |
| Apache-2.0 OR BSD-3-Clause | 2 | moxcms, pxfm |
| Apache-2.0 OR BSD-2-Clause OR MIT | 2 | zerocopy, zerocopy-derive |
| BSD-3-Clause (vendored native code) | libwebp | **libwebp 1.5.0** via libwebp-sys2 (builtin-vendored C) |
| 0BSD OR Apache-2.0 OR MIT | 1 | adler2 |
| MIT OR Unlicense | 1 | byteorder-lite |
| Zlib | 1 | zlib-rs |
| (Apache-2.0 OR MIT) AND Unicode-3.0 | 1 | unicode-ident |
| Apache-2.0 OR LGPL-2.1-or-later OR MIT | 1 | r-efi — compiles only on UEFI targets; not linked into macOS/Windows binaries |

## Redistribution duties

- **libwebp (BSD-3-Clause, Google, vendored 1.5.0):** its `COPYING`,
  `AUTHORS`, and `PATENTS` must ship with every CLI release and in the
  4D component `Resources/licenses/`.
- All other dependencies: standard MIT/Apache/BSD/Zlib notice retention in
  the release manifest (WP-03) is sufficient.
- No copyleft binaries are distributed.

## Hard pins required at first release (plan §11.5)

- Rust toolchain (`rust-toolchain.toml`, to be added in Phase 2)
- `Cargo.lock` committed
- libwebp version (from libwebp-sys2 vendored source)
- Per-target build flags, recorded in the release manifest

## Known cleanup

The prototype tree carries three libwebp bindings simultaneously
(`webp`→`libwebp-sys` 0.9.6, `webp-animation`→`libwebp-sys2` 0.1.11 and
0.2.0). The product workspace converges on a single sys crate (plan §3.5);
this table reflects the prototype, not the product.
