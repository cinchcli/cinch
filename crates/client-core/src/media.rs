//! Shared image-format detection by magic bytes.
//!
//! Single source of truth for the CLI, the desktop app, and any other
//! consumer. The same byte-sniffing previously lived — and drifted — in three
//! places: the CLI `push` pipeline (PNG/JPEG/GIF/WebP, missing TIFF/BMP), the
//! desktop `media` serving path (added TIFF/BMP), and the desktop image-save
//! path (its own PNG-default ext mapping). Consolidating them here keeps the
//! supported-format set identical everywhere.

/// A supported raster image format, identified by leading magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Tiff,
    Bmp,
}

impl ImageFormat {
    /// The canonical MIME type, e.g. `"image/png"`.
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Tiff => "image/tiff",
            ImageFormat::Bmp => "image/bmp",
        }
    }

    /// The conventional lowercase file extension (no dot), e.g. `"png"`.
    /// JPEG maps to `"jpg"`.
    pub fn ext(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Gif => "gif",
            ImageFormat::WebP => "webp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Bmp => "bmp",
        }
    }
}

/// Detect a supported image format from the leading magic bytes of `data`.
/// Returns `None` if the bytes don't match any known image signature.
pub fn detect_image_format(data: &[u8]) -> Option<ImageFormat> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some(ImageFormat::Jpeg)
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else if data.starts_with(b"II\x2a\x00") || data.starts_with(b"MM\x00\x2a") {
        Some(ImageFormat::Tiff)
    } else if data.starts_with(b"BM") && data.len() >= 14 && data[6..10] == [0, 0, 0, 0] {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

/// `true` if `data` begins with a supported image signature.
pub fn is_image(data: &[u8]) -> bool {
    detect_image_format(data).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_format() {
        assert_eq!(
            detect_image_format(b"\x89PNG\r\n\x1a\n\x00"),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            detect_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(detect_image_format(b"GIF89a..."), Some(ImageFormat::Gif));
        assert_eq!(detect_image_format(b"GIF87a..."), Some(ImageFormat::Gif));
        assert_eq!(
            detect_image_format(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(ImageFormat::WebP)
        );
        assert_eq!(
            detect_image_format(b"II\x2a\x00rest"),
            Some(ImageFormat::Tiff)
        );
        assert_eq!(
            detect_image_format(b"BM\x00\x00\x00\x00\x00\x00\x00\x00rest"),
            Some(ImageFormat::Bmp)
        );
    }

    #[test]
    fn rejects_non_images() {
        assert_eq!(detect_image_format(b"hello world"), None);
        assert_eq!(detect_image_format(b""), None);
        assert!(!is_image(b"plain text"));
    }

    #[test]
    fn mime_and_ext() {
        assert_eq!(ImageFormat::Png.mime(), "image/png");
        assert_eq!(ImageFormat::Jpeg.ext(), "jpg");
        assert_eq!(ImageFormat::WebP.mime(), "image/webp");
    }
}
