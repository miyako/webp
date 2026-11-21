//! Pixel-space operations owned by the policy layer: alpha flattening and
//! EXIF orientation math. Pure functions, fully unit-tested.

use crate::model::StillImage;

/// Composite `img` over an opaque RGB background (straight alpha).
/// Drops the alpha channel: the result is opaque.
pub fn flatten_background(img: &StillImage, rgb: [u8; 3]) -> StillImage {
    let mut rgba = Vec::with_capacity(img.rgba.len());
    for px in img.rgba.as_chunks::<4>().0 {
        let a = px[3] as u16;
        rgba.push(((px[0] as u16 * a + rgb[0] as u16 * (255 - a)) / 255) as u8);
        rgba.push(((px[1] as u16 * a + rgb[1] as u16 * (255 - a)) / 255) as u8);
        rgba.push(((px[2] as u16 * a + rgb[2] as u16 * (255 - a)) / 255) as u8);
        rgba.push(255);
    }
    StillImage {
        width: img.width,
        height: img.height,
        rgba,
        has_alpha: false,
        source: img.source.clone(),
    }
}

fn swap_w_h(w: u32, h: u32) -> (u32, u32) {
    (h, w)
}

fn rgba_index(w: u32, x: u32, y: u32) -> usize {
    ((y as u64 * w as u64 + x as u64) * 4) as usize
}

/// Apply one of the eight EXIF orientations as a pixel-space transform.
/// `1`, unknown (0) and 6/8-corrected results return the image unchanged for 1/0.
pub fn apply_orientation(img: &StillImage, orientation: u8) -> crate::Result<StillImage> {
    let (w, h) = (img.width, img.height);
    let src = &img.rgba;
    let build = |ow: u32, oh: u32, get: &dyn Fn(u32, u32) -> [u8; 4]| -> StillImage {
        let mut out = vec![0u8; (ow as u64 * oh as u64 * 4) as usize];
        for y in 0..oh {
            for x in 0..ow {
                let px = get(x, y);
                let i = rgba_index(ow, x, y);
                out[i..i + 4].copy_from_slice(&px);
            }
        }
        StillImage {
            width: ow,
            height: oh,
            rgba: out,
            has_alpha: img.has_alpha,
            source: img.source.clone(),
        }
    };

    match orientation {
        0 | 1 => Ok(StillImage {
            width: w,
            height: h,
            rgba: src.clone(),
            has_alpha: img.has_alpha,
            source: img.source.clone(),
        }),
        // 2: horizontal flip
        2 => Ok(build(w, h, &|x, y| {
            let i = rgba_index(w, w - 1 - x, y);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        // 3: rotate 180
        3 => Ok(build(w, h, &|x, y| {
            let i = rgba_index(w, w - 1 - x, h - 1 - y);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        // 4: vertical flip
        4 => Ok(build(w, h, &|x, y| {
            let i = rgba_index(w, x, h - 1 - y);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        // 5: transpose (flip along main diagonal)
        5 => Ok(build(swap_w_h(w, h).0, w, &|x, y| {
            let i = rgba_index(w, y, x);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        // 6: rotate 90 clockwise
        6 => Ok(build(swap_w_h(w, h).0, w, &|x, y| {
            let i = rgba_index(w, y, h - 1 - x);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        // 7: transverse (flip along anti-diagonal)
        7 => Ok(build(swap_w_h(w, h).0, w, &|x, y| {
            let i = rgba_index(w, w - 1 - y, h - 1 - x);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        // 8: rotate 90 counter-clockwise
        8 => Ok(build(swap_w_h(w, h).0, w, &|x, y| {
            let i = rgba_index(w, w - 1 - y, x);
            [src[i], src[i + 1], src[i + 2], src[i + 3]]
        })),
        n => Err(crate::WcError::CorruptInput(format!(
            "invalid EXIF orientation value {n}"
        ))),
    }
}

/// A pixel-space summary displayed after any orientation: whether stored
/// dimensions differ from display dimensions.
pub fn display_dimensions(stored: (u32, u32), orientation: u8) -> (u32, u32) {
    if matches!(orientation, 5..=8) {
        (stored.1, stored.0)
    } else {
        stored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &StillImage, x: u32, y: u32) -> [u8; 4] {
        let i = rgba_index(img.width, x, y);
        img.rgba[i..i + 4].try_into().unwrap()
    }

    fn img2x2() -> StillImage {
        // R G / B K
        StillImage::from_rgba8(
            2,
            2,
            vec![255, 0, 0, 200, 0, 255, 0, 200, 0, 0, 255, 200, 0, 0, 0, 200],
        )
        .unwrap()
    }

    #[test]
    fn flatten_is_opaque_white_blend() {
        let img = StillImage::from_rgba8(1, 1, vec![4, 8, 16, 128]).unwrap();
        let out = flatten_background(&img, [255, 255, 255]);
        // Over-composite: (fg*a + bg*(255-a))/255, exact integer math:
        // (4*128 + 255*127)/255 = 129, (8*128 + 255*127)/255 = 131,
        // (16*128 + 255*127)/255 = 135.
        assert_eq!(px(&out, 0, 0), [129, 131, 135, 255]);
        assert!(!out.has_alpha);
    }

    #[test]
    fn transparent_goes_to_background() {
        let img = StillImage::from_rgba8(1, 1, vec![4, 8, 16, 0]).unwrap();
        let out = flatten_background(&img, [10, 20, 30]);
        assert_eq!(px(&out, 0, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn orientations_1_to_4() {
        let base = img2x2();
        // 2: h-flip => G R / K B
        let o2 = apply_orientation(&base, 2).unwrap();
        assert_eq!(px(&o2, 0, 0)[0], 0);
        assert_eq!(px(&o2, 1, 0)[0], 255);
        // 3: 180 => K B / G R
        let o3 = apply_orientation(&base, 3).unwrap();
        assert_eq!(px(&o3, 0, 0), [0, 0, 0, 200]);
        assert_eq!(px(&o3, 1, 1), [255, 0, 0, 200]);
        // 4: v-flip => B K / R G
        let o4 = apply_orientation(&base, 4).unwrap();
        assert_eq!(px(&o4, 0, 0)[2], 255);
        // dims unchanged
        assert_eq!((o2.width, o2.height, o3.height, o4.height), (2, 2, 2, 2));
    }

    #[test]
    fn orientations_5_to_8_swap_dimensions() {
        let base = img2x2();
        for o in [5u8, 6, 7, 8] {
            let out = apply_orientation(&base, o).unwrap();
            assert_eq!((out.width, out.height), (2, 2)); // square fixture
        }
        // rectangular check with a 4x2 image (32 bytes of RGBA)
        let rec = StillImage::from_rgba8(4, 2, vec![1u8; 4 * 2 * 4]).unwrap();
        let out = apply_orientation(&rec, 6).unwrap();
        assert_eq!((out.width, out.height), (2, 4));
        assert!(apply_orientation(&rec, 9).is_err());
    }

    #[test]
    fn display_dims_flip_for_rotations() {
        assert_eq!(display_dimensions((400, 200), 1), (400, 200));
        assert_eq!(display_dimensions((400, 200), 6), (200, 400));
        assert_eq!(display_dimensions((400, 200), 8), (200, 400));
    }
}
