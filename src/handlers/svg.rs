//! Handler per conversione SVG

use std::path::Path;
use crate::error::{AppError, Result};

/// Converte SVG in formato raster (PNG, JPG, WebP, etc.)
pub fn convert_svg_to_raster(
    svg_data: &[u8],
    output_format: &str,
    width: Option<u32>,
    height: Option<u32>,
    quality: Option<u8>,
) -> Result<Vec<u8>> {
    // Parse SVG
    let svg_str = std::str::from_utf8(svg_data)
        .map_err(|e| AppError::ConversionError(format!("SVG non valido: {}", e)))?;

    let options = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg_str, &options)
        .map_err(|e| AppError::ConversionError(format!("Errore parsing SVG: {}", e)))?;

    // Calcola dimensioni output
    let svg_size = tree.size();
    let (out_width, out_height) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => {
            let ratio = w as f32 / svg_size.width();
            (w, (svg_size.height() * ratio) as u32)
        }
        (None, Some(h)) => {
            let ratio = h as f32 / svg_size.height();
            ((svg_size.width() * ratio) as u32, h)
        }
        (None, None) => (svg_size.width() as u32, svg_size.height() as u32),
    };

    // Crea pixmap per rendering
    let mut pixmap = tiny_skia::Pixmap::new(out_width, out_height)
        .ok_or_else(|| AppError::ConversionError("Impossibile creare pixmap".to_string()))?;

    // Calcola transform per scaling
    let scale_x = out_width as f32 / svg_size.width();
    let scale_y = out_height as f32 / svg_size.height();
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

    // Render SVG
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Converti in formato output
    let rgba_data = pixmap.data();
    let img = image::RgbaImage::from_raw(out_width, out_height, rgba_data.to_vec())
        .ok_or_else(|| AppError::ConversionError("Errore creazione immagine".to_string()))?;

    let dynamic_img = image::DynamicImage::ImageRgba8(img);

    // Encode nel formato richiesto
    encode_image(&dynamic_img, output_format, quality)
}

/// Converte file SVG in formato raster
pub fn convert_svg_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    width: Option<u32>,
    height: Option<u32>,
    quality: Option<u8>,
) -> Result<()> {
    let svg_data = std::fs::read(input_path)?;
    let output_data = convert_svg_to_raster(&svg_data, output_format, width, height, quality)?;
    std::fs::write(output_path, output_data)?;
    Ok(())
}

fn encode_image(img: &image::DynamicImage, format: &str, quality: Option<u8>) -> Result<Vec<u8>> {
    use std::io::Cursor;

    let mut buffer = Cursor::new(Vec::new());

    match format.to_lowercase().as_str() {
        "png" => {
            img.write_to(&mut buffer, image::ImageFormat::Png)?;
        }
        "jpg" | "jpeg" => {
            let q = quality.unwrap_or(85);
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, q);
            img.write_with_encoder(encoder)?;
        }
        "webp" => {
            img.write_to(&mut buffer, image::ImageFormat::WebP)?;
        }
        "gif" => {
            img.write_to(&mut buffer, image::ImageFormat::Gif)?;
        }
        "bmp" => {
            img.write_to(&mut buffer, image::ImageFormat::Bmp)?;
        }
        "avif" => {
            img.write_to(&mut buffer, image::ImageFormat::Avif)?;
        }
        "qoi" => {
            img.write_to(&mut buffer, image::ImageFormat::Qoi)?;
        }
        _ => {
            return Err(AppError::UnsupportedFormat(format!(
                "Formato output non supportato per SVG: {}",
                format
            )));
        }
    }

    Ok(buffer.into_inner())
}
