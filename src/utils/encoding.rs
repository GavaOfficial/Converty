//! Shared image encoding utilities

use image::{DynamicImage, ImageFormat};
use std::io::Cursor;

use crate::error::{AppError, Result};

/// Encode a DynamicImage to the specified format
///
/// # Arguments
/// * `img` - The image to encode
/// * `format` - Output format (png, jpg, jpeg, webp, gif, bmp, avif, qoi, tiff)
/// * `quality` - Optional quality for JPEG encoding (1-100, default 85)
///
/// # Returns
/// The encoded image bytes
pub fn encode_image(img: &DynamicImage, format: &str, quality: Option<u8>) -> Result<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());

    match format.to_lowercase().as_str() {
        "jpg" | "jpeg" => {
            let q = quality.unwrap_or(85);
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, q);
            img.write_with_encoder(encoder)?;
        }
        "png" => {
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut buffer,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            );
            img.write_with_encoder(encoder)?;
        }
        "webp" => {
            img.write_to(&mut buffer, ImageFormat::WebP)?;
        }
        "gif" => {
            img.write_to(&mut buffer, ImageFormat::Gif)?;
        }
        "bmp" => {
            img.write_to(&mut buffer, ImageFormat::Bmp)?;
        }
        "avif" => {
            img.write_to(&mut buffer, ImageFormat::Avif)?;
        }
        "qoi" => {
            img.write_to(&mut buffer, ImageFormat::Qoi)?;
        }
        "tiff" | "tif" => {
            img.write_to(&mut buffer, ImageFormat::Tiff)?;
        }
        _ => {
            return Err(AppError::UnsupportedFormat(format!(
                "Formato output non supportato: {}",
                format
            )));
        }
    }

    Ok(buffer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn create_test_image() -> DynamicImage {
        let img = RgbaImage::from_fn(10, 10, |_, _| image::Rgba([255, 0, 0, 255]));
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn test_encode_png() {
        let img = create_test_image();
        let result = encode_image(&img, "png", None);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_encode_jpeg() {
        let img = create_test_image();
        let result = encode_image(&img, "jpg", Some(80));
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_encode_unsupported() {
        let img = create_test_image();
        let result = encode_image(&img, "xyz", None);
        assert!(result.is_err());
    }
}
