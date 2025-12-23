use serde::Serialize;
use utoipa::ToSchema;

use super::JobStatus;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Stato dell'API
    pub status: String,
    /// Versione dell'API
    pub version: String,
    /// FFmpeg disponibile per conversione audio/video
    pub ffmpeg_available: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FormatsResponse {
    pub image: FormatSupport,
    pub svg: FormatSupport,
    pub document: FormatSupport,
    pub audio: FormatSupport,
    pub video: FormatSupport,
    pub pdf: FormatSupport,
    /// Limite massimo dimensione file in MB
    pub max_file_size_mb: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FormatSupport {
    pub input: Vec<String>,
    pub output: Vec<String>,
    /// Indica se questo tipo di conversione è disponibile (librerie installate)
    pub available: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConvertResponse {
    pub success: bool,
    pub original_name: String,
    pub original_format: String,
    pub output_format: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchConvertResponse {
    pub success: bool,
    pub converted: Vec<ConvertedFile>,
    pub failed: Vec<FailedFile>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ConvertedFile {
    pub original_name: String,
    pub output_format: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FailedFile {
    pub original_name: String,
    pub error: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JobResponse {
    pub id: String,
    pub status: JobStatus,
    pub conversion_type: String,
    pub input_format: String,
    pub output_format: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JobCreatedResponse {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub status: u16,
}
