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

/// The largest upload accepted, in bytes: room for a photo straight off a phone or camera.
/// Matches the limit the project form shows. Only the compressed result is stored.
pub const MAX_UPLOAD_BYTES: usize = 10_000_000;

/// Decoding refuses images wider or taller than this, whatever their file size.
const MAX_DECODED_DIMENSION: u32 = 8192;

/// Decoding refuses to allocate more than this for pixels. An 8192 square RGBA image is
/// 256 MiB; this is that bound.
const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;

/// Covers fill a tile a few hundred pixels wide and a project page banner about 1100 CSS
/// pixels wide. Anything larger is scaled down to fit inside this box, which keeps a
/// full-width banner sharp on a high-density screen. Smaller images are never scaled up.
const COVER_MAX_WIDTH: u32 = 2400;
const COVER_MAX_HEIGHT: u32 = 1350;

/// Lossy WebP quality, 0 to 100. Visually clean for photos and logos at a fraction of
/// PNG's size: a 2400 by 1350 photo compresses to a few hundred kilobytes.
const COVER_QUALITY: f32 = 80.0;

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

    let mut decoded = decode_bounded(upload)?;
    if decoded.width() > COVER_MAX_WIDTH || decoded.height() > COVER_MAX_HEIGHT {
        // `resize` keeps the aspect ratio and fits the image inside the box.
        decoded = decoded.resize(COVER_MAX_WIDTH, COVER_MAX_HEIGHT, FilterType::Lanczos3);
    }

    encode_webp(&decoded)
}

/// The largest still of a meeting's video taken, in bytes: a 4K screen share as a lossless PNG,
/// which is the most a browser's canvas makes of one frame.
pub const MAX_FRAME_UPLOAD_BYTES: usize = 25_000_000;

/// A frame is kept at most this size, which keeps every word of a shared slide or screen
/// readable while a meeting's frames stay small: 1080p, the most a meeting app shares.
const FRAME_MAX_WIDTH: u32 = 1920;
const FRAME_MAX_HEIGHT: u32 = 1080;

/// Lossy WebP quality for a frame, higher than a cover's: text on a slide must stay crisp, where
/// a photo forgives more.
const FRAME_QUALITY: f32 = 88.0;

/// The grid a fingerprint compares across: 17 columns by 16 rows gives 16 differences a row, 256
/// bits in all. Coarse enough that a frame cut a moment later, or encoded again, keeps its bits,
/// and fine enough that a slide with a line of text added does not.
const FINGERPRINT_COLUMNS: u32 = 17;
const FINGERPRINT_ROWS: u32 = 16;

/// How many of a fingerprint's 256 bits two frames may differ in and still be one picture: about
/// 3%, what the noise of encoding a video twice and a cursor moving across it change, while a
/// slide changed by a bullet or a new window differs in far more.
pub const NEAR_DUPLICATE_BITS: u32 = 8;

/// A still of a meeting's video as Elysium keeps it: lossy WebP from its pixels, at most
/// [`FRAME_MAX_WIDTH`] by [`FRAME_MAX_HEIGHT`], and its fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub webp: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub fingerprint: [u8; 32],
}

/// Decodes a still taken from a meeting's video, a browser's canvas or `ffmpeg` having made it,
/// and keeps it as a [`Frame`]: scaled to fit, re-encoded from its pixels, and fingerprinted.
///
/// CPU-bound: call it from a blocking task.
///
/// # Errors
/// [`ImageError::TooLarge`] past [`MAX_FRAME_UPLOAD_BYTES`], [`ImageError::Unsupported`] for
/// anything but a PNG, JPEG, WebP, or GIF image, [`ImageError::Undecodable`] for corrupt or
/// oversized images, and [`ImageError::Encoding`] if WebP encoding fails.
pub fn compress_frame(upload: &[u8]) -> Result<Frame, ImageError> {
    if upload.len() > MAX_FRAME_UPLOAD_BYTES {
        return Err(ImageError::TooLarge);
    }
    let mut decoded = decode_bounded(upload)?;
    if decoded.width() > FRAME_MAX_WIDTH || decoded.height() > FRAME_MAX_HEIGHT {
        decoded = decoded.resize(FRAME_MAX_WIDTH, FRAME_MAX_HEIGHT, FilterType::Lanczos3);
    }
    // A frame of a video is opaque: drawn on black, as the player shows it.
    let opaque = DynamicImage::ImageRgb8(decoded.to_rgb8());
    let webp = webp::Encoder::from_rgb(opaque.as_bytes(), opaque.width(), opaque.height())
        .encode_simple(false, FRAME_QUALITY)
        .map(|memory| memory.to_vec())
        .map_err(|error| ImageError::Encoding(format!("{error:?}")))?;
    Ok(Frame {
        webp,
        width: opaque.width(),
        height: opaque.height(),
        fingerprint: fingerprint(&opaque),
    })
}

/// A difference hash of `image`: shrunk to a [`FINGERPRINT_COLUMNS`] by [`FINGERPRINT_ROWS`]
/// grey grid, each bit whether brightness falls from one cell to the next along its row. Two
/// pictures of the same thing share it but for a few bits, whatever their size or encoding.
pub fn fingerprint(image: &DynamicImage) -> [u8; 32] {
    let grid = image
        .resize_exact(FINGERPRINT_COLUMNS, FINGERPRINT_ROWS, FilterType::Triangle)
        .to_luma8();
    let mut bits = [0_u8; 32];
    let mut index = 0_usize;
    for row in 0..FINGERPRINT_ROWS {
        for column in 0..FINGERPRINT_COLUMNS - 1 {
            let here = grid.get_pixel(column, row).0[0];
            let next = grid.get_pixel(column + 1, row).0[0];
            if here > next {
                bits[index / 8] |= 1 << (index % 8);
            }
            index += 1;
        }
    }
    bits
}

/// How many bits two fingerprints differ in.
pub fn fingerprint_distance(first: &[u8; 32], second: &[u8; 32]) -> u32 {
    first
        .iter()
        .zip(second)
        .map(|(left, right)| (left ^ right).count_ones())
        .sum()
}

/// Decodes `upload` as one of [`ACCEPTED_FORMATS`], read from its bytes, within the dimension and
/// memory bounds every upload is held to.
fn decode_bounded(upload: &[u8]) -> Result<DynamicImage, ImageError> {
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
    reader
        .decode()
        .map_err(|error| ImageError::Undecodable(error.to_string()))
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
        let banner = RgbaImage::from_pixel(4800, 1350, Rgba([30, 120, 200, 255]));
        let cover = decode_webp(&compress_cover(&png(&banner)).expect("compresses"));
        assert_eq!(cover.dimensions(), (2400, 675));
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

    /// A slide: a pale page with dark bars of "text" at each of `lines`, one bar a line.
    fn slide(lines: &[u32]) -> RgbaImage {
        let mut slide = RgbaImage::from_pixel(1280, 720, Rgba([240, 240, 235, 255]));
        for line in lines {
            for y in (100 + line * 60)..(130 + line * 60) {
                for x in 120..1000 {
                    slide.put_pixel(x, y, Rgba([20, 20, 40, 255]));
                }
            }
        }
        slide
    }

    #[test]
    fn a_frame_is_kept_as_webp_no_larger_than_1080p() {
        let screen = RgbaImage::from_pixel(3840, 2160, Rgba([10, 60, 120, 255]));
        let frame = compress_frame(&png(&screen)).expect("compresses");

        assert_eq!((frame.width, frame.height), (1920, 1080));
        assert_eq!(decode_webp(&frame.webp).dimensions(), (1920, 1080));
        assert!(matches!(
            compress_frame(b"<svg onload=alert(1)>"),
            Err(ImageError::Unsupported)
        ));
    }

    #[test]
    fn the_same_picture_encoded_again_is_a_near_duplicate_and_a_new_line_is_not() {
        let first = compress_frame(&png(&slide(&[0, 1, 2]))).expect("compresses");
        // The same slide as a lossy WebP of a smaller size, as a second capture might send it.
        let small = DynamicImage::ImageRgba8(slide(&[0, 1, 2])).resize(640, 360, FilterType::Nearest);
        let again = compress_frame(&encode_webp(&small).expect("encodes")).expect("compresses");
        let added = compress_frame(&png(&slide(&[0, 1, 2, 3]))).expect("compresses");

        let same = fingerprint_distance(&first.fingerprint, &again.fingerprint);
        let different = fingerprint_distance(&first.fingerprint, &added.fingerprint);
        assert!(same <= NEAR_DUPLICATE_BITS, "encoded again differs in {same} bits");
        assert!(different > NEAR_DUPLICATE_BITS, "a new line differs in {different} bits");
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
