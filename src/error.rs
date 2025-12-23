use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("File non trovato: {0}")]
    NotFound(String),

    #[error("Formato non supportato: {0}")]
    UnsupportedFormat(String),

    #[error("Errore di conversione: {0}")]
    ConversionError(String),

    #[error("File troppo grande: massimo {0} MB")]
    FileTooLarge(u64),

    #[error("Errore di I/O: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Errore immagine: {0}")]
    ImageError(#[from] image::ImageError),

    #[error("Campo multipart mancante: {0}")]
    MissingField(String),

    #[error("Job non trovato: {0}")]
    JobNotFound(String),

    #[error("Job non completato")]
    JobNotCompleted,

    #[error("FFmpeg non disponibile: {0}")]
    FfmpegError(String),

    #[error("Poppler non disponibile: {0}")]
    PopplerError(String),

    #[error("Non autorizzato: {0}")]
    Unauthorized(String),

    #[error("Accesso negato: {0}")]
    Forbidden(String),

    #[error("Troppe richieste: {0}")]
    RateLimited(String),

    #[error("Limite giornaliero raggiunto: {0}")]
    DailyLimitExceeded(String),

    #[error("Troppi job in coda: {0}")]
    TooManyJobs(String),

    #[error("Richiesta non valida: {0}")]
    BadRequest(String),

    #[error("Errore interno: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::UnsupportedFormat(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::ConversionError(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            AppError::FileTooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
            AppError::IoError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            AppError::ImageError(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            AppError::MissingField(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::JobNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::JobNotCompleted => (StatusCode::ACCEPTED, self.to_string()),
            AppError::FfmpegError(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            AppError::PopplerError(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::DailyLimitExceeded(_) => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::TooManyJobs(_) => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
