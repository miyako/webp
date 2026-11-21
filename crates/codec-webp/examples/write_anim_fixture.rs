//! Writes the committed animated-WebP fixture
//! `tests/fixtures/animated/webp/two_frames.webp` (4x4, red then green,
//! 200 ms apart). Run from the repo root:
//!
//! ```sh
//! cargo run -p webp-convert-codec-webp --example write_anim_fixture
//! ```

fn main() {
    // locate the repo root by walking up until tests/fixtures exists
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join("tests/fixtures").is_dir() {
            break;
        }
        assert!(dir.pop(), "repo root with tests/fixtures not found");
    }
    let path = dir.join("tests/fixtures/animated/webp/two_frames.webp");
    codec_webp::testing::write_two_frame_webp(&path).unwrap_or_else(|e| panic!("{e}"));
    println!("wrote {}", path.display());
}
