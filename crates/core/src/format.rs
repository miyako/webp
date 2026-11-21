//! Image format identifier and associated strings/extensions.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    WebP,
    Png,
    Jpeg,
    Gif,
    Bmp,
    Tiff,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 6] = [
        ImageFormat::WebP,
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Bmp,
        ImageFormat::Tiff,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ImageFormat::WebP => "webp",
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
        }
    }

    /// Canonical and accepted filename extensions (lowercased).
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            ImageFormat::WebP => &["webp"],
            ImageFormat::Png => &["png", "apng"],
            ImageFormat::Jpeg => &["jpg", "jpeg", "jfif"],
            ImageFormat::Gif => &["gif"],
            ImageFormat::Bmp => &["bmp", "dib"],
            ImageFormat::Tiff => &["tif", "tiff"],
        }
    }

    /// Does the format store multiple frames/pages?
    pub fn is_sequence_capable(&self) -> bool {
        matches!(
            self,
            ImageFormat::Gif | ImageFormat::WebP | ImageFormat::Tiff
        )
    }

    /// Does the format carry an alpha channel?
    pub fn supports_alpha(&self) -> bool {
        !matches!(self, ImageFormat::Jpeg | ImageFormat::Bmp)
    }

    pub fn image_format(&self) -> image::ImageFormat {
        match self {
            ImageFormat::WebP => image::ImageFormat::WebP,
            ImageFormat::Png => image::ImageFormat::Png,
            ImageFormat::Jpeg => image::ImageFormat::Jpeg,
            ImageFormat::Gif => image::ImageFormat::Gif,
            ImageFormat::Bmp => image::ImageFormat::Bmp,
            ImageFormat::Tiff => image::ImageFormat::Tiff,
        }
    }
}

impl fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ImageFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_ascii_lowercase();
        ImageFormat::ALL
            .into_iter()
            .find(|f| f.as_str() == s || f.extensions().contains(&s.as_str()))
            .ok_or_else(|| format!("unknown format {s:?}"))
    }
}

/// Content sniffing from magic bytes (never extension-based alone).
pub fn sniff(data: &[u8]) -> Option<ImageFormat> {
    if data.len() >= 8 && data[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some(ImageFormat::Png);
    }
    if data.len() >= 3 && data[..3] == [0xFF, 0xD8, 0xFF] {
        return Some(ImageFormat::Jpeg);
    }
    if data.len() >= 6 && (&data[..6] == b"GIF87a" || &data[..6] == b"GIF89a") {
        return Some(ImageFormat::Gif);
    }
    if data.len() > 6
        && &data[..2] == b"BM"
        // plausible BMP header size field
        && u32::from_le_bytes([data[2], data[3], data[4], data[5]]) > 0
    {
        return Some(ImageFormat::Bmp);
    }
    if data.len() >= 4 && (&data[..4] == b"II*\0" || &data[..4] == b"MM\0*") {
        return Some(ImageFormat::Tiff);
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some(ImageFormat::WebP);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_all_p0_formats() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n..."), Some(ImageFormat::Png));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(ImageFormat::Jpeg));
        assert_eq!(sniff(b"GIF89a...."), Some(ImageFormat::Gif));
        assert_eq!(sniff(b"II*\0\0\0"), Some(ImageFormat::Tiff));
        assert_eq!(sniff(b"MM\0*.."), Some(ImageFormat::Tiff));
        assert_eq!(
            sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(ImageFormat::WebP)
        );
    }

    #[test]
    fn rejects_non_images_and_weak_bmp() {
        assert_eq!(sniff(b"hello world, this is not an image"), None);
        assert_eq!(sniff(b""), None);
        // "BM" with implausible size field: rejected
        assert_eq!(sniff(b"BM\0\0\0\0rest"), None);
    }

    #[test]
    fn from_str_accepts_formats_and_extensions() {
        assert_eq!("webp".parse(), Ok(ImageFormat::WebP));
        assert_eq!("JPG".parse(), Ok(ImageFormat::Jpeg));
        assert_eq!("tif".parse(), Ok(ImageFormat::Tiff));
        assert!("xyz".parse::<ImageFormat>().is_err());
    }
}
