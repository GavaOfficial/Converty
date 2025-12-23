use std::path::Path;
use std::process::Command;

use crate::config::formats;
use crate::error::{AppError, Result};
use crate::utils::check_ffmpeg_available;

pub fn convert_audio(
    input_data: &[u8],
    input_format: &str,
    output_format: &str,
    quality: Option<u8>,
) -> Result<Vec<u8>> {
    if !check_ffmpeg_available() {
        return Err(AppError::FfmpegError(
            "FFmpeg non e' installato nel sistema".to_string(),
        ));
    }

    if !formats::is_supported_audio_input(input_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato audio input non supportato: {}",
            input_format
        )));
    }

    if !formats::is_supported_audio_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato audio output non supportato: {}",
            output_format
        )));
    }

    // Crea file temporanei
    let temp_dir = tempfile::tempdir()?;
    let input_path = temp_dir.path().join(format!("input.{}", input_format));
    let output_path = temp_dir.path().join(format!("output.{}", output_format));

    std::fs::write(&input_path, input_data)?;

    // Esegui conversione
    convert_audio_file(&input_path, &output_path, output_format, quality)?;

    // Leggi output
    let output_data = std::fs::read(&output_path)?;

    Ok(output_data)
}

pub fn convert_audio_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    quality: Option<u8>,
) -> Result<()> {
    if !check_ffmpeg_available() {
        return Err(AppError::FfmpegError(
            "FFmpeg non e' installato nel sistema".to_string(),
        ));
    }

    let mut args = vec![
        "-y", // Sovrascrivi output
        "-i",
        input_path.to_str().unwrap_or(""),
    ];

    // Aggiungi parametri qualita' per formato
    let quality_args: Vec<String> = match output_format.to_lowercase().as_str() {
        "mp3" => {
            let q = quality.unwrap_or(2); // 0-9, 0 = migliore
            vec!["-q:a".to_string(), q.to_string()]
        }
        "ogg" => {
            let q = quality.unwrap_or(5); // 0-10
            vec!["-q:a".to_string(), q.to_string()]
        }
        "flac" => {
            vec!["-compression_level".to_string(), "8".to_string()]
        }
        _ => {
            vec![]
        }
    };

    for arg in &quality_args {
        args.push(arg);
    }

    args.push(output_path.to_str().unwrap_or(""));

    run_ffmpeg_command(&args)
}

pub fn convert_video(
    input_data: &[u8],
    input_format: &str,
    output_format: &str,
    quality: Option<u8>,
) -> Result<Vec<u8>> {
    if !check_ffmpeg_available() {
        return Err(AppError::FfmpegError(
            "FFmpeg non e' installato nel sistema".to_string(),
        ));
    }

    if !formats::is_supported_video_input(input_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato video input non supportato: {}",
            input_format
        )));
    }

    if !formats::is_supported_video_output(output_format) {
        return Err(AppError::UnsupportedFormat(format!(
            "Formato video output non supportato: {}",
            output_format
        )));
    }

    // Crea file temporanei
    let temp_dir = tempfile::tempdir()?;
    let input_path = temp_dir.path().join(format!("input.{}", input_format));
    let output_path = temp_dir.path().join(format!("output.{}", output_format));

    std::fs::write(&input_path, input_data)?;

    // Esegui conversione
    convert_video_file(&input_path, &output_path, output_format, quality)?;

    // Leggi output
    let output_data = std::fs::read(&output_path)?;

    Ok(output_data)
}

pub fn convert_video_file(
    input_path: &Path,
    output_path: &Path,
    output_format: &str,
    quality: Option<u8>,
) -> Result<()> {
    if !check_ffmpeg_available() {
        return Err(AppError::FfmpegError(
            "FFmpeg non e' installato nel sistema".to_string(),
        ));
    }

    let mut args = vec!["-y", "-i", input_path.to_str().unwrap_or("")];

    // Parametri specifici per formato
    let format_args: Vec<String> = match output_format.to_lowercase().as_str() {
        "mp4" => {
            let crf = quality.map(|q| 51 - (q as i32 * 51 / 100)).unwrap_or(23);
            vec![
                "-c:v".to_string(),
                "libx264".to_string(),
                "-crf".to_string(),
                crf.to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
            ]
        }
        "webm" => {
            let crf = quality.map(|q| 63 - (q as i32 * 63 / 100)).unwrap_or(30);
            vec![
                "-c:v".to_string(),
                "libvpx-vp9".to_string(),
                "-crf".to_string(),
                crf.to_string(),
                "-b:v".to_string(),
                "0".to_string(),
                "-c:a".to_string(),
                "libopus".to_string(),
            ]
        }
        "avi" => {
            vec![
                "-c:v".to_string(),
                "mpeg4".to_string(),
                "-c:a".to_string(),
                "mp3".to_string(),
            ]
        }
        "gif" => {
            // Conversione speciale per GIF animata
            vec![
                "-vf".to_string(),
                "fps=10,scale=320:-1:flags=lanczos".to_string(),
                "-loop".to_string(),
                "0".to_string(),
            ]
        }
        _ => {
            vec![]
        }
    };

    for arg in &format_args {
        args.push(arg);
    }

    args.push(output_path.to_str().unwrap_or(""));

    run_ffmpeg_command(&args)
}

fn run_ffmpeg_command(args: &[&str]) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .map_err(|e| AppError::FfmpegError(format!("Impossibile eseguire ffmpeg: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::FfmpegError(format!("FFmpeg fallito: {}", stderr)));
    }

    Ok(())
}
