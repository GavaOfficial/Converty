use std::path::Path;
use std::process::Command;

use crate::error::{AppError, Result};

pub fn get_extension(filename: &str) -> Option<String> {
    Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase())
}

pub fn get_mime_type(filename: &str) -> String {
    mime_guess::from_path(filename)
        .first_or_octet_stream()
        .to_string()
}

pub fn check_ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn check_pdftoppm_available() -> bool {
    Command::new("pdftoppm")
        .arg("-v")
        .output()
        .map(|_| true) // pdftoppm -v outputs to stderr with exit 0
        .unwrap_or(false)
}

pub fn run_ffmpeg(args: &[&str]) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .map_err(|e| AppError::FfmpegError(format!("Impossibile eseguire ffmpeg: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::FfmpegError(format!(
            "FFmpeg fallito: {}",
            stderr
        )));
    }

    Ok(())
}

pub fn validate_file_size(size: u64, max_size_mb: u64) -> Result<()> {
    let max_bytes = max_size_mb * 1024 * 1024;
    if size > max_bytes {
        return Err(AppError::FileTooLarge(max_size_mb));
    }
    Ok(())
}
