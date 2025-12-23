use std::path::Path;

use crate::config::formats;
use crate::error::Result;
use crate::handlers::{document, image, media, pdf, svg};
use crate::models::ConversionType;

pub fn convert(
    data: &[u8],
    input_format: &str,
    output_format: &str,
    conversion_type: &ConversionType,
    quality: Option<u8>,
) -> Result<Vec<u8>> {
    // Gestione speciale per SVG
    if formats::is_svg_input(input_format) {
        return svg::convert_svg_to_raster(data, output_format, None, None, quality);
    }

    // Gestione speciale per PDF - converte tutte le pagine in ZIP se multi-pagina
    if formats::is_pdf_input(input_format) {
        let page_count = pdf::get_pdf_page_count(data).unwrap_or(1);
        if page_count > 1 {
            return pdf::convert_pdf_to_zip(data, output_format, None, "pages");
        } else {
            return pdf::convert_pdf_to_image(data, output_format, None, None);
        }
    }

    match conversion_type {
        ConversionType::Image => image::convert_image_with_quality(data, input_format, output_format, quality),
        ConversionType::Document => document::convert_document(data, input_format, output_format),
        ConversionType::Audio => media::convert_audio(data, input_format, output_format, quality),
        ConversionType::Video => media::convert_video(data, input_format, output_format, quality),
        ConversionType::Pdf => {
            let page_count = pdf::get_pdf_page_count(data).unwrap_or(1);
            if page_count > 1 {
                pdf::convert_pdf_to_zip(data, output_format, None, "pages")
            } else {
                pdf::convert_pdf_to_image(data, output_format, None, None)
            }
        }
    }
}

/// Converte un file PDF in immagini. Restituisce true se il risultato è uno ZIP (multi-pagina).
pub fn convert_pdf_file_smart(
    input_path: &Path,
    output_dir: &Path,
    output_format: &str,
) -> Result<(std::path::PathBuf, bool)> {
    let data = std::fs::read(input_path)?;
    let page_count = pdf::get_pdf_page_count(&data).unwrap_or(1);

    let base_name = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    if page_count > 1 {
        // Multi-page: crea ZIP
        let zip_data = pdf::convert_pdf_to_zip(&data, output_format, None, base_name)?;
        let output_path = output_dir.join("output.zip");
        std::fs::write(&output_path, zip_data)?;
        Ok((output_path, true))
    } else {
        // Single page: crea singola immagine
        let output_path = output_dir.join(format!("output.{}", output_format));
        pdf::convert_pdf_file(input_path, &output_path, output_format, None, None)?;
        Ok((output_path, false))
    }
}

pub fn convert_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    conversion_type: &ConversionType,
    quality: Option<u8>,
) -> Result<()> {
    // Gestione speciale per SVG
    let input_ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if formats::is_svg_input(input_ext) {
        return svg::convert_svg_file(input_path, output_path, output_format, None, None, quality);
    }

    // Gestione speciale per PDF - converte tutte le pagine
    if formats::is_pdf_input(input_ext) {
        let data = std::fs::read(input_path)?;
        let page_count = pdf::get_pdf_page_count(&data).unwrap_or(1);

        if page_count > 1 {
            // Multi-page: crea ZIP nella stessa directory con estensione .zip
            let base_name = input_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output");
            let zip_data = pdf::convert_pdf_to_zip(&data, output_format, None, base_name)?;
            let zip_path = output_path.with_extension("zip");
            std::fs::write(&zip_path, zip_data)?;
            // Crea anche un file marker col path originale per il download
            std::fs::write(output_path.with_extension("zip.marker"), zip_path.to_string_lossy().as_bytes())?;
            return Ok(());
        } else {
            return pdf::convert_pdf_file(input_path, output_path, output_format, None, None);
        }
    }

    match conversion_type {
        ConversionType::Image => {
            image::convert_image_file(input_path, output_path, output_format, quality)
        }
        ConversionType::Document => {
            document::convert_document_file(input_path, output_path, output_format)
        }
        ConversionType::Audio => {
            media::convert_audio_file(input_path, output_path, output_format, quality)
        }
        ConversionType::Video => {
            media::convert_video_file(input_path, output_path, output_format, quality)
        }
        ConversionType::Pdf => {
            let data = std::fs::read(input_path)?;
            let page_count = pdf::get_pdf_page_count(&data).unwrap_or(1);

            if page_count > 1 {
                let base_name = input_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");
                let zip_data = pdf::convert_pdf_to_zip(&data, output_format, None, base_name)?;
                let zip_path = output_path.with_extension("zip");
                std::fs::write(&zip_path, zip_data)?;
                Ok(())
            } else {
                pdf::convert_pdf_file(input_path, output_path, output_format, None, None)
            }
        }
    }
}

pub fn detect_conversion_type(extension: &str) -> Option<ConversionType> {
    let ext = extension.to_lowercase();

    // SVG è trattato come Image per il tipo di conversione
    if formats::is_svg_input(&ext) {
        Some(ConversionType::Image)
    } else if formats::is_pdf_input(&ext) {
        Some(ConversionType::Pdf)
    } else if formats::is_supported_image_input(&ext) {
        Some(ConversionType::Image)
    } else if formats::is_supported_document_input(&ext) {
        Some(ConversionType::Document)
    } else if formats::is_supported_audio_input(&ext) {
        Some(ConversionType::Audio)
    } else if formats::is_supported_video_input(&ext) {
        Some(ConversionType::Video)
    } else {
        None
    }
}
