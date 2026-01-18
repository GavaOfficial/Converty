//! Validation utilities for format and tool checking

use crate::config::formats;
use crate::error::{AppError, Result};
use crate::utils::file::{check_ffmpeg_available, check_pdftoppm_available};

/// Direction of format conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatDirection {
    Input,
    Output,
}

/// Category of format for validation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatCategory {
    Image,
    Document,
    Audio,
    Video,
    Pdf,
}

/// External tool required for conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTool {
    Ffmpeg,
    Pdftoppm,
}

/// Validate that a format is supported for the given category and direction
///
/// # Arguments
/// * `format` - The format to validate
/// * `category` - The type of file (image, document, etc.)
/// * `direction` - Whether this is input or output format
///
/// # Returns
/// Ok(()) if valid, Err with appropriate message if not
pub fn validate_format(
    format: &str,
    category: FormatCategory,
    direction: FormatDirection,
) -> Result<()> {
    let is_valid = match (category, direction) {
        (FormatCategory::Image, FormatDirection::Input) => {
            formats::is_supported_image_input(format)
        }
        (FormatCategory::Image, FormatDirection::Output) => {
            formats::is_supported_image_output(format)
        }
        (FormatCategory::Document, FormatDirection::Input) => {
            formats::is_supported_document_input(format)
        }
        (FormatCategory::Document, FormatDirection::Output) => {
            formats::is_supported_document_output(format)
        }
        (FormatCategory::Audio, FormatDirection::Input) => {
            formats::is_supported_audio_input(format)
        }
        (FormatCategory::Audio, FormatDirection::Output) => {
            formats::is_supported_audio_output(format)
        }
        (FormatCategory::Video, FormatDirection::Input) => {
            formats::is_supported_video_input(format)
        }
        (FormatCategory::Video, FormatDirection::Output) => {
            formats::is_supported_video_output(format)
        }
        (FormatCategory::Pdf, FormatDirection::Input) => format.to_lowercase() == "pdf",
        (FormatCategory::Pdf, FormatDirection::Output) => {
            formats::is_supported_image_output(format)
        }
    };

    if !is_valid {
        let direction_str = match direction {
            FormatDirection::Input => "input",
            FormatDirection::Output => "output",
        };
        return Err(AppError::UnsupportedFormat(format!(
            "Formato {} non supportato: {}",
            direction_str, format
        )));
    }

    Ok(())
}

/// Check if an external tool is available
///
/// # Arguments
/// * `tool` - The tool to check
///
/// # Returns
/// Ok(()) if available, Err with appropriate message if not
pub fn validate_tool_available(tool: ExternalTool) -> Result<()> {
    match tool {
        ExternalTool::Ffmpeg => {
            if !check_ffmpeg_available() {
                return Err(AppError::FfmpegError(
                    "FFmpeg non è disponibile sul sistema".to_string(),
                ));
            }
        }
        ExternalTool::Pdftoppm => {
            if !check_pdftoppm_available() {
                return Err(AppError::PopplerError(
                    "pdftoppm non è disponibile sul sistema".to_string(),
                ));
            }
        }
    }

    Ok(())
}

/// Validate both input and output formats for a conversion
///
/// # Arguments
/// * `input_format` - The input format
/// * `output_format` - The output format
/// * `category` - The type of conversion
///
/// # Returns
/// Ok(()) if both are valid, Err with appropriate message if not
pub fn validate_conversion_formats(
    input_format: &str,
    output_format: &str,
    category: FormatCategory,
) -> Result<()> {
    validate_format(input_format, category, FormatDirection::Input)?;
    validate_format(output_format, category, FormatDirection::Output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_image_formats() {
        assert!(validate_format("png", FormatCategory::Image, FormatDirection::Input).is_ok());
        assert!(validate_format("jpg", FormatCategory::Image, FormatDirection::Output).is_ok());
        assert!(validate_format("xyz", FormatCategory::Image, FormatDirection::Input).is_err());
    }

    #[test]
    fn test_validate_conversion_formats() {
        assert!(validate_conversion_formats("png", "jpg", FormatCategory::Image).is_ok());
    }
}
