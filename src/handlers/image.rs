use image::{DynamicImage, ImageFormat};
use std::path::Path;

use crate::config::formats;
use crate::error::{AppError, Result};
use crate::models::ImageOptions;
use crate::utils::encode_image;

pub fn convert_image(
    input_data: &[u8],
    input_format: &str,
    output_format: &str,
    options: &ImageOptions,
) -> Result<Vec<u8>> {
    // Valida formati
    if !formats::is_supported_image_input(input_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato input non supportato: {}",
            input_format
        )));
    }

    if !formats::is_supported_image_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato output non supportato: {}",
            output_format
        )));
    }

    // Carica immagine
    let mut img = image::load_from_memory(input_data)?;

    // Applica resize se richiesto
    img = apply_resize(img, options);

    // Converti nel formato di output
    let output_data = encode_image(&img, output_format, options.quality)?;

    Ok(output_data)
}

pub fn convert_image_with_quality(
    input_data: &[u8],
    input_format: &str,
    output_format: &str,
    quality: Option<u8>,
) -> Result<Vec<u8>> {
    let options = ImageOptions {
        quality,
        ..Default::default()
    };
    convert_image(input_data, input_format, output_format, &options)
}

pub fn convert_image_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    quality: Option<u8>,
) -> Result<()> {
    let input_format = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if !formats::is_supported_image_input(input_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato input non supportato: {}",
            input_format
        )));
    }

    if !formats::is_supported_image_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato output non supportato: {}",
            output_format
        )));
    }

    let img = image::open(input_path)?;

    match output_format.to_lowercase().as_str() {
        "jpg" | "jpeg" => {
            let q = quality.unwrap_or(85);
            img.save_with_format(output_path, ImageFormat::Jpeg)
                .map_err(|e| AppError::ConversionError(e.to_string()))?;
            // Per JPEG con qualita' specifica, usiamo l'encoder diretto
            if quality.is_some() {
                let mut output = std::fs::File::create(output_path)?;
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, q);
                img.write_with_encoder(encoder)?;
            }
        }
        _ => {
            let format = get_image_format(output_format)?;
            img.save_with_format(output_path, format)?;
        }
    }

    Ok(())
}

/// Applica resize all'immagine
fn apply_resize(img: DynamicImage, options: &ImageOptions) -> DynamicImage {
    match (options.width, options.height) {
        (Some(w), Some(h)) => {
            if options.maintain_aspect_ratio {
                // Resize mantenendo proporzioni (fit inside box)
                img.resize(w, h, image::imageops::FilterType::Lanczos3)
            } else {
                // Resize esatto (può distorcere)
                img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
            }
        }
        (Some(w), None) => {
            // Solo larghezza: calcola altezza proporzionale
            let ratio = w as f32 / img.width() as f32;
            let h = (img.height() as f32 * ratio) as u32;
            img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        }
        (None, Some(h)) => {
            // Solo altezza: calcola larghezza proporzionale
            let ratio = h as f32 / img.height() as f32;
            let w = (img.width() as f32 * ratio) as u32;
            img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        }
        (None, None) => img, // Nessun resize
    }
}

fn get_image_format(format: &str) -> Result<ImageFormat> {
    match format.to_lowercase().as_str() {
        "png" => Ok(ImageFormat::Png),
        "jpg" | "jpeg" => Ok(ImageFormat::Jpeg),
        "gif" => Ok(ImageFormat::Gif),
        "bmp" => Ok(ImageFormat::Bmp),
        "webp" => Ok(ImageFormat::WebP),
        "tiff" => Ok(ImageFormat::Tiff),
        "ico" => Ok(ImageFormat::Ico),
        _ => Err(AppError::UnsupportedFormat(format!(
            "Formato non supportato: {}",
            format
        ))),
    }
}

/// Ottieni info sull'immagine
pub fn get_image_info(data: &[u8]) -> Result<ImageInfo> {
    let img = image::load_from_memory(data)?;
    Ok(ImageInfo {
        width: img.width(),
        height: img.height(),
        color_type: format!("{:?}", img.color()),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub color_type: String,
}
