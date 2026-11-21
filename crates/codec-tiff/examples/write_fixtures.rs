//! Writes the committed Phase-5 TIFF fixtures:
//!
//! - `tests/fixtures/multipage/tiff/two_pages.tiff` — a two-page TIFF
//!   (page 0: 8x8 red RGB8; page 1: 8x8 blue RGBA8)
//! - `tests/fixtures/color/16bit/gray16.tiff` — a 4x4 16-bit grayscale TIFF
//!
//! Run from the repo root:
//!
//! ```sh
//! cargo run -p webp-convert-codec-tiff --example write_fixtures
//! ```

use std::fs::File;
use std::io::BufWriter;
use tiff::encoder::colortype::{Gray16, RGB8, RGBA8};
use tiff::encoder::TiffEncoder;

fn repo_root() -> std::path::PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join("tests/fixtures").is_dir() {
            return dir;
        }
        assert!(dir.pop(), "repo root with tests/fixtures not found");
    }
}

fn write_two_pages(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    let mut tiff = TiffEncoder::new(BufWriter::new(file))?;

    // page 0: solid red 8x8 RGB8
    let mut red_px = Vec::with_capacity(8 * 8 * 3);
    for _ in 0..8 * 8 {
        red_px.extend_from_slice(&[255, 0, 0]);
    }
    tiff.write_image::<RGB8>(8, 8, &red_px)?;

    // page 1: solid blue 8x8 RGBA8 (opaque; distinct from page 0)
    let mut blue_px = Vec::with_capacity(8 * 8 * 4);
    for _ in 0..8 * 8 {
        blue_px.extend_from_slice(&[0, 0, 255, 255]);
    }
    tiff.write_image::<RGBA8>(8, 8, &blue_px)?;

    Ok(())
}

fn write_gray16(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    let mut tiff = TiffEncoder::new(BufWriter::new(file))?;
    // 4x4 with a visible ramp: 0, ~21845 (1/3), ~43690 (2/3), 65535
    let mut px = Vec::with_capacity(16);
    for i in 0..16u16 {
        px.push(i * 4369);
    }
    tiff.write_image::<Gray16>(4, 4, &px)?;
    Ok(())
}

fn main() {
    let root = repo_root();
    let mp = root.join("tests/fixtures/multipage/tiff/two_pages.tiff");
    let g16 = root.join("tests/fixtures/color/16bit/gray16.tiff");
    write_two_pages(&mp).unwrap_or_else(|e| panic!("two_pages.tiff: {e}"));
    write_gray16(&g16).unwrap_or_else(|e| panic!("gray16.tiff: {e}"));
    println!("wrote {}", mp.display());
    println!("wrote {}", g16.display());
}
