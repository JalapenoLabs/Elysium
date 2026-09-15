// Copyright © 2026 Jalapeno Labs

//! Turns uploaded images into what Elysium stores and serves.
//!
//! Uploads are untrusted: the format is read from the bytes rather than a declared
//! content type, and decoding is bounded in dimensions and memory, so a small file that
//! claims to be enormous is refused before it is expanded. The result is re-encoded from
//! pixels, which also drops metadata such as camera location.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};

/// The largest upload accepted, in bytes. Matches the limit the settings page shows.
pub const MAX_UPLOAD_BYTES: usize = 1_000_000;

/// Decoding refuses images wider or taller than this, whatever their file size.
const MAX_DECODED_DIMENSION: u32 = 8192;

/// Decoding refuses to allocate more than this for pixels. An 8192 square RGBA image is
/// 256 MiB; this is that bound.
const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;

/// Covers are shown in a 16:9 frame at most a few hundred pixels wide. Anything larger is
/// scaled down to fit inside this box, which still covers a full-width banner on a
/// high-density screen. Smaller images are never scaled up.
const COVER_MAX_WIDTH: u32 = 1600;
const COVER_MAX_HEIGHT: u32 = 900;

/// Lossy WebP quality, 0 to 100. Visually clean for photos and logos at a fraction of
/// PNG's size.
const COVER_QUALITY: f32 = 82.0;

/// Formats browsers commonly produce and the `image` build here decodes.
const ACCEPTED_FORMATS: [ImageFormat; 4] = [
    ImageFormat::Png,
    ImageFormat::Jpeg,
    ImageFormat::WebP,
    ImageFormat::Gif,
];

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("the image is larger than {MAX_UPLOAD_BYTES} bytes")]
    TooLarge,
    #[error("the file is not a PNG, JPEG, WebP, or GIF image")]
    Unsupported,
    #[error("the image could not be read: {0}")]
    Undecodable(String),
    #[error("the image could not be compressed: {0}")]
    Encoding(String),
}

/// Decodes an uploaded cover, scales it to fit [`COVER_MAX_WIDTH`] by
/// [`COVER_MAX_HEIGHT`], and encodes it as lossy WebP, keeping transparency. An animated
/// GIF keeps its first frame.
///
/// CPU-bound: call it from a blocking task.
///
/// # Errors
/// [`ImageError::TooLarge`] and [`ImageError::Unsupported`] for uploads refused before
/// decoding, [`ImageError::Undecodable`] for corrupt or oversized images, and
/// [`ImageError::Encoding`] if WebP encoding fails.
pub fn compress_cover(upload: &[u8]) -> Result<Vec<u8>, ImageError> {
    if upload.len() > MAX_UPLOAD_BYTES {
        return Err(ImageError::TooLarge);
    }

    let mut reader = ImageReader::new(Cursor::new(upload))
        .with_guessed_format()
        .map_err(|error| ImageError::Undecodable(error.to_string()))?;
    if !reader
        .format()
        .is_some_and(|format| ACCEPTED_FORMATS.contains(&format))
    {
        return Err(ImageError::Unsupported);
    }

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_DIMENSION);
    limits.max_image_height = Some(MAX_DECODED_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);

    let mut decoded = reader
        .decode()
        .map_err(|error| ImageError::Undecodable(error.to_string()))?;
    if decoded.width() > COVER_MAX_WIDTH || decoded.height() > COVER_MAX_HEIGHT {
        // `resize` keeps the aspect ratio and fits the image inside the box.
        decoded = decoded.resize(COVER_MAX_WIDTH, COVER_MAX_HEIGHT, FilterType::Lanczos3);
    }

    encode_webp(&decoded)
}

fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>, ImageError> {
    let rgba = image.to_rgba8();
    // An alpha channel that is opaque everywhere costs bytes and says nothing.
    let is_opaque = rgba.pixels().all(|pixel| pixel.0[3] == u8::MAX);

    let encoded = if is_opaque {
        let rgb = image.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
            .encode_simple(false, COVER_QUALITY)
    } else {
        webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
            .encode_simple(false, COVER_QUALITY)
    };

    encoded
        .map(|memory| memory.to_vec())
        .map_err(|error| ImageError::Encoding(format!("{error:?}")))
}

#[cfg(test)]
mod tests {
    use image::{GenericImageView, ImageEncoder, Rgba, RgbaImage};

    use super::*;

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .expect("png encodes");
        bytes
    }

    fn decode_webp(bytes: &[u8]) -> DynamicImage {
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        image::load_from_memory_with_format(bytes, ImageFormat::WebP).expect("webp decodes")
    }

    #[test]
    fn a_large_banner_is_scaled_to_fit_and_keeps_its_shape() {
        let banner = RgbaImage::from_pixel(3200, 900, Rgba([30, 120, 200, 255]));
        let cover = decode_webp(&compress_cover(&png(&banner)).expect("compresses"));
        assert_eq!(cover.dimensions(), (1600, 450));
    }

    #[test]
    fn a_small_square_logo_is_not_scaled_up_and_keeps_transparency() {
        let mut logo = RgbaImage::from_pixel(256, 256, Rgba([0, 0, 0, 0]));
        for pixel in logo.pixels_mut().take(256 * 64) {
            *pixel = Rgba([255, 80, 0, 255]);
        }
        let cover = decode_webp(&compress_cover(&png(&logo)).expect("compresses"));
        assert_eq!(cover.dimensions(), (256, 256));
        assert!(cover.color().has_alpha());
        assert_eq!(cover.get_pixel(255, 255).0[3], 0);
    }

    #[test]
    fn uploads_that_are_not_accepted_images_are_refused() {
        assert!(matches!(
            compress_cover(b"%PDF-1.7 not an image"),
            Err(ImageError::Unsupported)
        ));
        assert!(matches!(
            compress_cover(&vec![0_u8; MAX_UPLOAD_BYTES + 1]),
            Err(ImageError::TooLarge)
        ));

        let mut truncated = png(&RgbaImage::from_pixel(64, 64, Rgba([1, 2, 3, 255])));
        truncated.truncate(truncated.len() / 2);
        assert!(matches!(
            compress_cover(&truncated),
            Err(ImageError::Undecodable(_))
        ));
    }

    /// The CRC-32 a PNG chunk carries over its type and data.
    fn png_chunk_crc(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _bit in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn an_image_claiming_huge_dimensions_is_refused_before_decoding() {
        // A well-formed 1 by 1 PNG whose header is rewritten to declare 20000 by 20000.
        let mut forged = png(&RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])));
        // IHDR's type starts at byte 12, its width and height at 16, its CRC at 29.
        forged[16..20].copy_from_slice(&20_000_u32.to_be_bytes());
        forged[20..24].copy_from_slice(&20_000_u32.to_be_bytes());
        let crc = png_chunk_crc(&forged[12..29]);
        forged[29..33].copy_from_slice(&crc.to_be_bytes());

        let refusal = compress_cover(&forged).expect_err("refused");
        assert!(
            matches!(&refusal, ImageError::Undecodable(reason) if reason.contains("limit")),
            "refused by the dimension limit, not a format error: {refusal}"
        );
    }
}
