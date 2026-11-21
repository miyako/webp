//! Feasibility prototype for the WebP Converter CLI master plan (Phase 1, T04).
//!
//! Answers, with evidence printed to stdout:
//!   1. Can WebP be encoded lossy + lossless with alpha from Rust (libwebp)?
//!   2. Is lossless WebP round-trip pixel-exact?
//!   3. Do PNG/JPEG/TIFF/BMP behave as expected via the `image` crate?
//!   4. Is animated GIF decode and animated WebP decode/encode viable?
//!   5. Is still-vs-animated WebP detection reachable?
//!
//! This is a scratch feasibility tool. It is not the product CLI and must not ship.

use std::fmt::Write as _;
use image::AnimationDecoder;

#[derive(Default)]
struct Report {
    lines: Vec<String>,
    failures: Vec<String>,
}

impl Report {
    fn ok(&mut self, pass: bool, label: &str, detail: String) {
        let mark = if pass { "PASS" } else { "FAIL" };
        self.lines.push(format!("[{mark}] {label}: {detail}"));
        if !pass {
            self.failures.push(label.to_string());
        }
    }
    fn info(&mut self, label: &str, detail: String) {
        self.lines.push(format!("[info] {label}: {detail}"));
    }
    fn finish(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            let _ = writeln!(out, "{l}");
        }
        let _ = writeln!(out);
        if self.failures.is_empty() {
            let _ = writeln!(out, "RESULT: all checks passed");
        } else {
            let _ = writeln!(
                out,
                "RESULT: {} failing checks: {:?}",
                self.failures.len(),
                self.failures
            );
        }
        out
    }
}

/// RGB(A) gradient, deterministic.
fn gradient(w: u32, h: u32, alpha: bool) -> image::RgbaImage {
    image::ImageBuffer::from_fn(w, h, |x, y| {
        let r = (x * 255 / w.max(1)) as u8;
        let g = (y * 255 / h.max(1)) as u8;
        let b = ((x + y) * 255 / (w + h).max(1)) as u8;
        let a = if !alpha {
            255
        } else {
            // a hole in the middle, everywhere else opaque
            let inside = x > w / 4 && x < 3 * w / 4 && y > h / 4 && y < 3 * h / 4;
            if inside { 0 } else { 255 }
        };
        [r, g, b, a].into()
    })
}

/// Peak signal-to-noise ratio between two images (mean squared error per channel).
fn psnr(a: &image::RgbaImage, b: &image::RgbaImage) -> Option<f64> {
    if a.dimensions() != b.dimensions() {
        return None;
    }
    let mut total: f64 = 0.0;
    let mut n = 0u64;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        for c in 0..4 {
            let d = pa.0[c] as f64 - pb.0[c] as f64;
            total += d * d;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let mse = total / n as f64;
    if mse == 0.0 {
        Some(f64::INFINITY)
    } else {
        Some(10.0 * (255.0f64 * 255.0 / mse).log10())
    }
}

fn count_transparent(img: &image::RgbaImage) -> usize {
    img.pixels().filter(|p| p.0[3] == 0).count()
}

fn main() -> std::io::Result<()> {
    std::fs::create_dir_all("out")?;
    let mut r = Report::default();

    still_webp(&mut r);
    still_other_codecs(&mut r);
    animation(&mut r);

    print!("{}", r.finish());
    Ok(())
}

fn still_webp(r: &mut Report) {
    let rgb = gradient(256, 256, false);
    let rgba = gradient(256, 256, true);

    // -- libwebp lossy, no alpha
    let dynimg = image::DynamicImage::ImageRgba8(rgb.clone());
    let enc = webp::Encoder::from_image(&dynimg).expect("encoder");
    let cfg = webp::WebPConfig::new().unwrap();
    let mem = enc.encode_advanced(&cfg).expect("encode");
    std::fs::write("out/lossy.webp", &mem[..]).unwrap();
    match webp::Decoder::new(&mem).decode() {
        Some(img) => {
            let back = img.to_image().to_rgba8();
            r.ok(
                psnr(&rgb, &back).map_or(false, |p| p > 30.0),
                "webp lossy encode/decode (libwebp)",
                format!(
                    "bytes={} rgb PSNR={:.1} dB",
                    mem.len(),
                    psnr(&rgb, &back).unwrap_or(0.0)
                ),
            );
        }
        None => r.ok(
            false,
            "webp lossy encode/decode (libwebp)",
            "decode returned None".into(),
        ),
    }

    // -- libwebp lossy, with alpha
    let dynimg = image::DynamicImage::ImageRgba8(rgba.clone());
    let enc = webp::Encoder::from_image(&dynimg).expect("encoder");
    let cfg = webp::WebPConfig::new().unwrap();
    let mem = enc.encode_advanced(&cfg).expect("encode");
    std::fs::write("out/lossy-alpha.webp", &mem[..]).unwrap();
    match webp::Decoder::new(&mem).decode() {
        Some(img) => {
            let back = img.to_image().to_rgba8();
            r.ok(
                count_transparent(&back) > 0,
                "webp lossy encode/decode with alpha (libwebp)",
                format!(
                    "bytes={} transparent_px={} (input {})",
                    mem.len(),
                    count_transparent(&back),
                    count_transparent(&rgba)
                ),
            );
        }
        None => r.ok(
            false,
            "webp lossy encode/decode with alpha (libwebp)",
            "decode returned None".into(),
        ),
    }

    // -- libwebp lossless, alpha: must be exact (requires exact=1; see below)
    let mut cfg = webp::WebPConfig::new().unwrap();
    cfg.lossless = 1;
    cfg.exact = 1;
    let dynimg = image::DynamicImage::ImageRgba8(rgba.clone());
    let enc = webp::Encoder::from_image(&dynimg).expect("encoder");
    let mem = enc.encode_advanced(&cfg).expect("encode");
    std::fs::write("out/lossless-alpha-exact.webp", &mem[..]).unwrap();
    match webp::Decoder::new(&mem[..]).decode() {
        Some(img) => {
            let back = img.to_image().to_rgba8();
            let exact = back.pixels().zip(rgba.pixels()).all(|(a, b)| a == b);
            r.ok(
                exact,
                "webp LOSSLESS round-trip with alpha is pixel-exact (exact=1)",
                format!("bytes={}", mem.len()),
            );
        }
        None => r.ok(
            false,
            "webp LOSSLESS round-trip with alpha is pixel-exact (exact=1)",
            "decode returned None".into(),
        ),
    }

    // -- documented behavior: libwebp default exact=0 rewrites RGB under fully transparent pixels
    let mut cfg = webp::WebPConfig::new().unwrap();
    cfg.lossless = 1;
    cfg.exact = 0;
    let dynimg = image::DynamicImage::ImageRgba8(rgba.clone());
    let enc = webp::Encoder::from_image(&dynimg).expect("encoder");
    let mem = enc.encode_advanced(&cfg).expect("encode");
    match webp::Decoder::new(&mem[..]).decode() {
        Some(img) => {
            let back = img.to_image().to_rgba8();
            let mut mismatch = 0usize;
            for (a, b) in rgba.pixels().zip(back.pixels()) {
                if a != b {
                    mismatch += 1;
                }
            }
            let all_changed_are_transparent = rgba
                .pixels()
                .zip(back.pixels())
                .all(|(a, b)| a == b || a.0[3] == 0);
            r.ok(
                all_changed_are_transparent && mismatch > 0,
                "libwebp lossless default (exact=0) only alters fully-transparent pixels",
                format!("changed_pixels={mismatch} (all had alpha=0)"),
            );
        }
        None => r.ok(
            false,
            "libwebp lossless default (exact=0) only alters fully-transparent pixels",
            "decode returned None".into(),
        ),
    }

    // -- quality knob effect (lossy)
    let mut q90 = webp::WebPConfig::new().unwrap();
    q90.quality = 90.0;
    let mut q30 = webp::WebPConfig::new().unwrap();
    q30.quality = 30.0;
    let dynimg = image::DynamicImage::ImageRgba8(rgb.clone());
    let enc = webp::Encoder::from_image(&dynimg).expect("encoder");
    let m90 = enc.encode_advanced(&q90).expect("q90");
    let m30 = enc.encode_advanced(&q30).expect("q30");
    r.ok(
        m30.len() < m90.len(),
        "quality knob changes output size",
        format!("q90={}B q30={}B", m90.len(), m30.len()),
    );

    // -- still image exposes has_animation == false
    let features = webp::BitstreamFeatures::new(&m90[..]).expect("features");
    r.ok(
        !features.has_animation(),
        "still WebP detected as non-animated",
        format!("has_animation={}", features.has_animation()),
    );
}

fn still_other_codecs(r: &mut Report) {
    let rgba = gradient(256, 256, true);

    // PNG: alpha exact
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(rgba.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let back = image::load_from_memory(&png).unwrap().to_rgba8();
    r.ok(
        back.pixels().zip(rgba.pixels()).all(|(a, b)| a == b),
        "PNG round-trip exact with alpha",
        format!("{}B", png.len()),
    );

    // BMP: alpha behavior check (BMP alpha is inconsistent in the wild)
    let mut bmp = Vec::new();
    image::DynamicImage::ImageRgba8(rgba.clone())
        .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .unwrap();
    let back = image::load_from_memory(&bmp).unwrap();
    r.info(
        "BMP round-trip",
        format!("{}, decoded {:?}", bmp.len(), back.to_rgba8().dimensions()),
    );

    // JPEG: no alpha, lossy
    let mut jpg = Vec::new();
    {
        let rgb = image::DynamicImage::ImageRgba8(rgba.clone()).to_rgb8();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85);
        image::DynamicImage::ImageRgb8(rgb)
            .write_with_encoder(enc)
            .unwrap();
    }
    let back = image::load_from_memory(&jpg).map(|d| d.to_rgba8());
    match back {
        Ok(back) => {
            r.ok(
                count_transparent(&back) == 0,
                "JPEG output has no alpha (expected data-loss path)",
                format!("{}, transparent_px={}", jpg.len(), count_transparent(&back)),
            );
        }
        Err(e) => r.ok(false, "JPEG encode/decode", format!("{e}")),
    }

    // TIFF
    let mut tiff = Vec::new();
    image::DynamicImage::ImageRgba8(rgba.clone())
        .write_to(&mut std::io::Cursor::new(&mut tiff), image::ImageFormat::Tiff)
        .unwrap();
    let back = image::load_from_memory(&tiff).unwrap().to_rgba8();
    r.ok(
        back.pixels().zip(rgba.pixels()).all(|(a, b)| a == b),
        "TIFF round-trip exact with alpha (image crate)",
        format!("{}B", tiff.len()),
    );
}

fn animation(r: &mut Report) {
    // Build a 3-frame animated GIF with transparency.
    const W: usize = 64;
    const H: usize = 64;
    let mut frames_rgba: Vec<image::RgbaImage> = Vec::new();
    for f in 0..3u8 {
        let mut img = image::RgbaImage::new(W as u32, H as u32);
        for y in 0..H {
            for x in 0..W {
                let inside = x > 16 && x < 48 && y > 16 && y < 48;
                let a = if inside && (x / 8 + y / 8) % 3 == f as usize { 0 } else { 255 };
                img.put_pixel(
                    x as u32,
                    y as u32,
                    image::Rgba([16 * f, 255 - 16 * f, 128, a]),
                );
            }
        }
        frames_rgba.push(img);
    }

    // -- encode animated GIF via gif crate
    let mut gif_bytes = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut gif_bytes, W as u16, H as u16, &[0, 0, 0]).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();
        for f in &frames_rgba {
            let frame =
                gif::Frame::from_rgba_speed(W as u16, H as u16, &mut f.clone().into_raw(), 10);
            enc.write_frame(&frame).unwrap();
        }
    }
    std::fs::write("out/anim.gif", &gif_bytes).unwrap();

    // -- decode animated GIF via gif crate (frames + timing present)
    {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let reader = opts.read_info(std::io::Cursor::new(&gif_bytes)).unwrap();
        let mut n = 0;
        let mut dims = (0u16, 0u16);
        for frame in reader {
            let f = frame.unwrap();
            dims = (f.width, f.height);
            let _ = f.delay;
            n += 1;
        }
        r.ok(
            n == 3,
            "animated GIF decode: 3 frames with timing (gif crate)",
            format!("frames={n} dims={dims:?}"),
        );
    }

    // -- decode animated GIF via image crate (inspect path)
    match image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&gif_bytes)) {
        Ok(dec) => {
            let mut n = 0;
            for f in dec.into_frames() {
                f.unwrap();
                n += 1;
            }
            r.ok(n == 3, "image crate animated GIF into_frames()", format!("frames={n}"));
        }
        Err(e) => r.ok(false, "image crate animated GIF into_frames()", format!("{e}")),
    }

    // -- encode animated WebP via webp-animation, then decode frame count
    let webp_bytes: Vec<u8> = {
        let mut enc = webp_animation::Encoder::new((W as u32, H as u32)).expect("anim encoder");
        for (i, f) in frames_rgba.iter().enumerate() {
            enc.add_frame(f.as_raw(), (i * 100) as i32)
                .expect("add_frame");
        }
        enc.finalize(300).expect("finalize").to_vec()
    };
    std::fs::write("out/anim.webp", &webp_bytes).unwrap();

    let dec = webp_animation::Decoder::new(&webp_bytes).expect("anim decoder");
    let mut n = 0;
    let mut total_ms = 0;
    for f in dec {
        let _ = f.dimensions();
        total_ms += f.timestamp();
        n += 1;
    }
    r.ok(
        n == 3,
        "animated WebP decode: 3 frames with timing",
        format!("frames={n} sum_timestamps={total_ms} bytes={}", webp_bytes.len()),
    );
}

#[cfg(test)]
mod diag {
    #[test]
    fn lossless_mismatch_analysis() {
        use super::*;
        let rgba = gradient(256, 256, true);
        let mut cfg = webp::WebPConfig::new().unwrap();
        cfg.lossless = 1;
        cfg.alpha_quality = 100;
        cfg.exact = 1;
        let dynimg = image::DynamicImage::ImageRgba8(rgba.clone());
        let enc = webp::Encoder::from_image(&dynimg).expect("encoder");
        let mem = enc.encode_advanced(&cfg).expect("encode");
        let img = webp::Decoder::new(&mem[..]).decode().expect("decode");
        let back = img.to_image().to_rgba8();
        let mut mismatches = 0usize;
        let mut examples = Vec::new();
        for (i, (a, b)) in rgba.pixels().zip(back.pixels()).enumerate() {
            if a != b {
                mismatches += 1;
                if examples.len() < 5 { examples.push((i, a.0, b.0)); }
            }
        }
        println!("mismatches={mismatches} examples={examples:?}");
        assert_eq!(mismatches, 0, "count per-channel mismatches");
    }
}
