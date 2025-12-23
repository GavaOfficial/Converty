use printpdf::*;
use std::io::BufWriter;
use std::path::Path;

use crate::config::formats;
use crate::error::{AppError, Result};

pub fn convert_document(
    input_data: &[u8],
    input_format: &str,
    output_format: &str,
) -> Result<Vec<u8>> {
    if !formats::is_supported_document_input(input_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato input non supportato: {}",
            input_format
        )));
    }

    if !formats::is_supported_document_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato output non supportato: {}",
            output_format
        )));
    }

    let content = String::from_utf8_lossy(input_data).to_string();

    match output_format.to_lowercase().as_str() {
        "pdf" => text_to_pdf(&content, input_format),
        "txt" => Ok(extract_text(&content, input_format).into_bytes()),
        "html" => Ok(to_html(&content, input_format).into_bytes()),
        _ => Err(AppError::UnsupportedFormat(format!(
            "Formato output non supportato: {}",
            output_format
        ))),
    }
}

pub fn convert_document_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
) -> Result<()> {
    let input_format = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt");

    let content = std::fs::read_to_string(input_path)?;

    match output_format.to_lowercase().as_str() {
        "pdf" => {
            let pdf_data = text_to_pdf(&content, input_format)?;
            std::fs::write(output_path, pdf_data)?;
        }
        "txt" => {
            let text = extract_text(&content, input_format);
            std::fs::write(output_path, text)?;
        }
        "html" => {
            let html = to_html(&content, input_format);
            std::fs::write(output_path, html)?;
        }
        _ => {
            return Err(AppError::UnsupportedFormat(format!(
                "Formato output non supportato: {}",
                output_format
            )));
        }
    }

    Ok(())
}

fn text_to_pdf(content: &str, input_format: &str) -> Result<Vec<u8>> {
    let text = match input_format {
        "md" | "markdown" => markdown_to_text(content),
        "html" | "htm" => html_to_text(content),
        _ => content.to_string(),
    };

    // Crea documento PDF
    let (doc, page1, layer1) =
        PdfDocument::new("Documento Convertito", Mm(210.0), Mm(297.0), "Layer 1");

    let current_layer = doc.get_page(page1).get_layer(layer1);

    // Usa font built-in
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| AppError::ConversionError(e.to_string()))?;

    // Dividi il testo in righe e scrivi
    let lines: Vec<&str> = text.lines().collect();
    let mut y_position = 280.0; // Inizia dall'alto
    let line_height = 5.0;
    let margin_left = 20.0;
    let font_size = 12.0;

    for line in lines {
        if y_position < 20.0 {
            // Nuova pagina se necessario
            break; // Per semplicita', limitiamo a una pagina
        }

        current_layer.use_text(line, font_size, Mm(margin_left), Mm(y_position), &font);
        y_position -= line_height;
    }

    // Salva in memoria
    let mut buffer = BufWriter::new(Vec::new());
    doc.save(&mut buffer)
        .map_err(|e| AppError::ConversionError(e.to_string()))?;

    Ok(buffer.into_inner().map_err(|e| AppError::IoError(e.into_error()))?)
}

fn markdown_to_text(content: &str) -> String {
    // Conversione semplice: rimuovi sintassi markdown base
    let mut result = content.to_string();

    // Rimuovi headers
    result = result
        .lines()
        .map(|line| {
            if line.starts_with('#') {
                line.trim_start_matches('#').trim()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Rimuovi bold/italic
    result = result.replace("**", "");
    result = result.replace("__", "");
    result = result.replace('*', "");
    result = result.replace('_', " ");

    // Rimuovi link syntax
    let re_link = regex_lite::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap_or_else(|_| {
        // Fallback se regex fallisce
        return regex_lite::Regex::new(r"").unwrap();
    });
    result = re_link.replace_all(&result, "$1").to_string();

    result
}

fn html_to_text(content: &str) -> String {
    // Rimozione semplice dei tag HTML
    let mut result = String::new();
    let mut in_tag = false;

    for ch in content.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Decodifica entita' HTML comuni
    result = result.replace("&nbsp;", " ");
    result = result.replace("&lt;", "<");
    result = result.replace("&gt;", ">");
    result = result.replace("&amp;", "&");
    result = result.replace("&quot;", "\"");

    result
}

fn extract_text(content: &str, input_format: &str) -> String {
    match input_format {
        "md" | "markdown" => markdown_to_text(content),
        "html" | "htm" => html_to_text(content),
        _ => content.to_string(),
    }
}

fn to_html(content: &str, input_format: &str) -> String {
    match input_format {
        "md" | "markdown" => markdown_to_html(content),
        "html" | "htm" => content.to_string(),
        _ => text_to_html(content),
    }
}

fn markdown_to_html(content: &str) -> String {
    // Conversione markdown semplice
    let mut html = String::from("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"UTF-8\">\n</head>\n<body>\n");

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("### ") {
            html.push_str(&format!("<h3>{}</h3>\n", &trimmed[4..]));
        } else if trimmed.starts_with("## ") {
            html.push_str(&format!("<h2>{}</h2>\n", &trimmed[3..]));
        } else if trimmed.starts_with("# ") {
            html.push_str(&format!("<h1>{}</h1>\n", &trimmed[2..]));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            html.push_str(&format!("<li>{}</li>\n", &trimmed[2..]));
        } else if trimmed.is_empty() {
            html.push_str("<br>\n");
        } else {
            html.push_str(&format!("<p>{}</p>\n", trimmed));
        }
    }

    html.push_str("</body>\n</html>");
    html
}

fn text_to_html(content: &str) -> String {
    let mut html = String::from("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"UTF-8\">\n</head>\n<body>\n<pre>\n");
    html.push_str(&content.replace('<', "&lt;").replace('>', "&gt;"));
    html.push_str("\n</pre>\n</body>\n</html>");
    html
}
