//! Writes the committed Phase-6 metadata fixtures (plan §5.6 write-through
//! tests): real ICC / EXIF / XMP payloads embedded in real JPEG and PNG
//! containers. Run from the repo root:
//!
//! ```sh
//! cargo run -p webp-convert-core --example write_metadata_fixtures
//! ```

use std::io::Write as _;

fn repo_root() -> std::path::PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join("tests/fixtures").is_dir() {
            return dir;
        }
        assert!(dir.pop(), "repo root with tests/fixtures not found");
    }
}

/// A tiny, byte-consistent but non-standard ICC profile placeholder: the
/// conversion tool carries profile bytes opaquely, so exactness matters,
/// real-color validity does not (documented in caps/restrictions).
fn fake_icc() -> Vec<u8> {
    let mut p = b"FAKE-ICC-PROFILE-WEBP-CONVERT-TEST-DEADBEEF".to_vec();
    // ICC payloads start with a 32-bit BE total size; make it consistent so
    // tools that validate size at least don't see garbage
    let size = p.len() as u32 + 4;
    let mut full = size.to_be_bytes().to_vec();
    full.append(&mut p);
    full
}

/// Minimal little-endian EXIF/TIFF with a single IFD0 entry:
/// Orientation (0x0112) = `value`.
fn exif_tiff_with_orientation(value: u16) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(b"II"); // LE
    t.extend_from_slice(&42u16.to_le_bytes());
    t.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8
    t.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
    t.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    t.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    t.extend_from_slice(&1u32.to_le_bytes()); // count
    t.extend_from_slice(&value.to_le_bytes());
    t.extend_from_slice(&[0, 0]); // inline SHORT padding
    t.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    t
}

const XMP: &[u8] = b"<?xpacket begin=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'>wc-test</x:xmpmeta><?xpacket end='w'?>";

fn crc32(data: &[u8]) -> u32 {
    // minimal CRC-32 (PNG chunk CRCs)
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        table[n as usize] = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn png_chunk(ctype: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut c = Vec::with_capacity(12 + body.len());
    c.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let mut crc_input = ctype.to_vec();
    crc_input.extend_from_slice(body);
    c.extend_from_slice(ctype);
    c.extend_from_slice(body);
    c.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    c
}

fn write_fixture_pngs(dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    // base PNG via the image crate (8x8, red/green halves)
    let mut base = Vec::new();
    {
        let mut img = image::RgbImage::new(8, 8);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 4 {
                [255, 0, 0].into()
            } else {
                [0, 255, 0].into()
            };
        }
        image::DynamicImage::ImageRgb8(img).write_to(
            &mut std::io::Cursor::new(&mut base),
            image::ImageFormat::Png,
        )?;
    }

    // --- PNG with iCCP + eXIf + iTXt XMP ---
    // split base into preamble chunks and IEND; insert custom chunks before IEND
    let iend_pos = base.windows(8).position(|w| &w[4..8] == b"IEND").unwrap();
    let mut bytes = base[..iend_pos].to_vec();

    // iCCP: keyword\0 compression-method(0) deflate(profile)
    let icc = fake_icc();
    let mut iccp = b"ICC\x00\x00".to_vec();
    {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&icc)?;
        iccp.extend(z.finish()?);
    }
    bytes.extend_from_slice(&png_chunk(b"iCCP", &iccp));
    bytes.extend_from_slice(&png_chunk(b"eXIf", &exif_tiff_with_orientation(6)));
    let itxt = {
        let mut t = b"XML:com.adobe.xmp\0\0\0".to_vec(); // keyword, no compress
        t.extend_from_slice(b"x-default\0\0"); // language, translated
        t.extend_from_slice(XMP);
        t
    };
    bytes.extend_from_slice(&png_chunk(b"iTXt", &itxt));
    bytes.extend_from_slice(&base[iend_pos..]);
    std::fs::write(
        dir.join("metadata/exif").join("png_icc_exif_xmp.png"),
        &bytes,
    )?;

    // --- JPEG with APP2 ICC + APP1 EXIF + APP1 XMP (inserted after SOI) ---
    let mut jbase = Vec::new();
    {
        let mut img = image::RgbImage::new(8, 8);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 4 {
                [255, 0, 0].into()
            } else {
                [0, 255, 0].into()
            };
        }
        image::DynamicImage::ImageRgb8(img).write_to(
            &mut std::io::Cursor::new(&mut jbase),
            image::ImageFormat::Jpeg,
        )?;
    }
    let icc = fake_icc();
    let mut app2 = b"ICC_PROFILE\0".to_vec();
    app2.push(1); // sequence 1
    app2.push(1); // count 1
    app2.extend_from_slice(&icc);
    let mut seg2 = vec![0xFF, 0xE2];
    seg2.extend_from_slice(&((app2.len() + 2) as u16).to_be_bytes());
    seg2.extend_from_slice(&app2);

    let exif = exif_tiff_with_orientation(6);
    let mut app1e = b"Exif\0\0".to_vec();
    app1e.extend_from_slice(&exif);
    let mut seg1e = vec![0xFF, 0xE1];
    seg1e.extend_from_slice(&((app1e.len() + 2) as u16).to_be_bytes());
    seg1e.extend_from_slice(&app1e);

    let mut app1x = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    app1x.extend_from_slice(XMP);
    let mut seg1x = vec![0xFF, 0xE1];
    seg1x.extend_from_slice(&((app1x.len() + 2) as u16).to_be_bytes());
    seg1x.extend_from_slice(&app1x);

    let mut jout = jbase[..2].to_vec(); // SOI
    jout.extend_from_slice(&seg2);
    jout.extend_from_slice(&seg1e);
    jout.extend_from_slice(&seg1x);
    jout.extend_from_slice(&jbase[2..]);
    std::fs::write(
        dir.join("metadata/exif").join("jpg_icc_exif_xmp.jpg"),
        &jout,
    )?;

    // --- orientation-only PNG (EXIF orientation 6, no other metadata) ---
    let iend_pos = base.windows(8).position(|w| &w[4..8] == b"IEND").unwrap();
    let mut ori = base[..iend_pos].to_vec();
    ori.extend_from_slice(&png_chunk(b"eXIf", &exif_tiff_with_orientation(6)));
    ori.extend_from_slice(&base[iend_pos..]);
    std::fs::write(dir.join("metadata/orientation").join("exif6.png"), &ori)?;

    Ok(())
}

fn main() {
    let root = repo_root();
    let fx = root.join("tests/fixtures");
    write_fixture_pngs(&fx).unwrap_or_else(|e| panic!("{e}"));
    println!("wrote metadata fixtures under {}", fx.display());
}
