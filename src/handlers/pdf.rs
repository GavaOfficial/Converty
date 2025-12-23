use std::io::{Cursor, Write};
use std::path::Path;
use std::process::Command;

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::config::formats;
use crate::error::{AppError, Result};
use crate::utils::check_pdftoppm_available;

/// Converte un PDF in immagine usando pdftoppm (poppler-utils)
pub fn convert_pdf_to_image(
    input_data: &[u8],
    output_format: &str,
    page: Option<u32>,
    dpi: Option<u32>,
) -> Result<Vec<u8>> {
    if !check_pdftoppm_available() {
        return Err(AppError::PopplerError(
            "pdftoppm (poppler-utils) non e' installato nel sistema".to_string(),
        ));
    }

    if !formats::is_supported_pdf_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato output non supportato per PDF: {}. Formati supportati: png, jpg, tiff",
            output_format
        )));
    }

    // Crea file temporanei
    let temp_dir = tempfile::tempdir()?;
    let input_path = temp_dir.path().join("input.pdf");
    let output_prefix = temp_dir.path().join("output");

    std::fs::write(&input_path, input_data)?;

    // Esegui conversione
    let output_path = run_pdftoppm(
        &input_path,
        &output_prefix,
        output_format,
        page.unwrap_or(1),
        dpi.unwrap_or(150),
    )?;

    // Leggi output
    let output_data = std::fs::read(&output_path)?;

    Ok(output_data)
}

/// Converte un PDF file in immagine
pub fn convert_pdf_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    page: Option<u32>,
    dpi: Option<u32>,
) -> Result<()> {
    if !check_pdftoppm_available() {
        return Err(AppError::PopplerError(
            "pdftoppm (poppler-utils) non e' installato nel sistema".to_string(),
        ));
    }

    if !formats::is_supported_pdf_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato output non supportato per PDF: {}",
            output_format
        )));
    }

    let temp_dir = tempfile::tempdir()?;
    let output_prefix = temp_dir.path().join("output");

    // Esegui conversione
    let temp_output = run_pdftoppm(
        input_path,
        &output_prefix,
        output_format,
        page.unwrap_or(1),
        dpi.unwrap_or(150),
    )?;

    // Copia al path finale
    std::fs::copy(&temp_output, output_path)?;

    Ok(())
}

/// Esegue pdftoppm e restituisce il path del file output
fn run_pdftoppm(
    input_path: &Path,
    output_prefix: &Path,
    output_format: &str,
    page: u32,
    dpi: u32,
) -> Result<std::path::PathBuf> {
    let format_arg = match output_format.to_lowercase().as_str() {
        "png" => "-png",
        "jpg" | "jpeg" => "-jpeg",
        "tiff" => "-tiff",
        _ => {
            return Err(AppError::UnsupportedFormat(format!(
                "Formato non supportato: {}",
                output_format
            )))
        }
    };

    let page_str = page.to_string();
    let dpi_str = dpi.to_string();

    let args = vec![
        "-f", &page_str,      // Prima pagina
        "-l", &page_str,      // Ultima pagina (stessa = singola pagina)
        "-r", &dpi_str,       // DPI
        "-singlefile",        // Non aggiunge suffisso numerico
        format_arg,           // Formato output
        input_path.to_str().unwrap_or(""),
        output_prefix.to_str().unwrap_or(""),
    ];

    let output = Command::new("pdftoppm")
        .args(&args)
        .output()
        .map_err(|e| AppError::PopplerError(format!("Impossibile eseguire pdftoppm: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::PopplerError(format!(
            "pdftoppm fallito: {}",
            stderr
        )));
    }

    // Determina estensione output
    let ext = match output_format.to_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        "tiff" => "tif",
        _ => output_format,
    };

    // pdftoppm con -singlefile crea: output_prefix.ext
    let output_path = output_prefix.with_extension(ext);

    if !output_path.exists() {
        return Err(AppError::PopplerError(
            "File output non generato da pdftoppm".to_string(),
        ));
    }

    Ok(output_path)
}

/// Ottiene il numero di pagine di un PDF
pub fn get_pdf_page_count(input_data: &[u8]) -> Result<u32> {
    let temp_dir = tempfile::tempdir()?;
    let input_path = temp_dir.path().join("input.pdf");
    std::fs::write(&input_path, input_data)?;

    // Usa pdfinfo per ottenere il numero di pagine
    let output = Command::new("pdfinfo")
        .arg(input_path.to_str().unwrap_or(""))
        .output()
        .map_err(|e| AppError::PopplerError(format!("Impossibile eseguire pdfinfo: {}", e)))?;

    if !output.status.success() {
        return Err(AppError::PopplerError(
            "pdfinfo fallito".to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Cerca la riga "Pages: N"
    for line in stdout.lines() {
        if line.starts_with("Pages:") {
            if let Some(count_str) = line.split_whitespace().nth(1) {
                if let Ok(count) = count_str.parse::<u32>() {
                    return Ok(count);
                }
            }
        }
    }

    // Default a 1 se non trovato
    Ok(1)
}

/// Converte tutte le pagine di un PDF in immagini (restituisce lista di file)
pub fn convert_pdf_all_pages(
    input_data: &[u8],
    output_format: &str,
    dpi: Option<u32>,
) -> Result<Vec<(String, Vec<u8>)>> {
    if !check_pdftoppm_available() {
        return Err(AppError::PopplerError(
            "pdftoppm (poppler-utils) non e' installato nel sistema".to_string(),
        ));
    }

    let page_count = get_pdf_page_count(input_data)?;
    let mut pages = Vec::new();

    for page in 1..=page_count {
        let data = convert_pdf_to_image(input_data, output_format, Some(page), dpi)?;
        let filename = format!("page_{:03}.{}", page, output_format);
        pages.push((filename, data));
    }

    Ok(pages)
}

/// Converte tutte le pagine di un PDF in un archivio ZIP contenente le immagini
pub fn convert_pdf_to_zip(
    input_data: &[u8],
    output_format: &str,
    dpi: Option<u32>,
    base_name: &str,
) -> Result<Vec<u8>> {
    // Converti tutte le pagine
    let pages = convert_pdf_all_pages(input_data, output_format, dpi)?;

    // Crea ZIP in memoria
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(6));

        for (filename, data) in pages {
            // Usa il nome base del PDF come prefisso cartella
            let path_in_zip = format!("{}/{}", base_name, filename);
            zip.start_file(&path_in_zip, options)
                .map_err(|e| AppError::Internal(format!("Errore creazione ZIP: {}", e)))?;
            zip.write_all(&data)
                .map_err(|e| AppError::Internal(format!("Errore scrittura ZIP: {}", e)))?;
        }

        zip.finish()
            .map_err(|e| AppError::Internal(format!("Errore finalizzazione ZIP: {}", e)))?;
    }

    Ok(buffer.into_inner())
}
