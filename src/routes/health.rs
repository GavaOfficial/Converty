use axum::{extract::State, routing::get, Json, Router};

use crate::config::formats;
use crate::models::{FormatSupport, FormatsResponse, HealthResponse};
use crate::utils::{check_ffmpeg_available, check_pdftoppm_available};

#[derive(Clone)]
pub struct HealthState {
    pub max_file_size_mb: u64,
}

pub fn router(max_file_size_mb: u64) -> Router {
    let state = HealthState { max_file_size_mb };
    Router::new()
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/formats", get(get_formats))
        .with_state(state)
}

/// Health check dell'API
#[utoipa::path(
    get,
    path = "/api/v1/health",
    responses(
        (status = 200, description = "API funzionante", body = HealthResponse),
    ),
    tag = "Sistema"
)]
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        ffmpeg_available: check_ffmpeg_available(),
    })
}

/// Ottieni i formati supportati (filtra in base alle librerie disponibili)
#[utoipa::path(
    get,
    path = "/api/v1/formats",
    responses(
        (status = 200, description = "Lista formati supportati", body = FormatsResponse),
    ),
    tag = "Sistema"
)]
pub async fn get_formats(State(state): State<HealthState>) -> Json<FormatsResponse> {
    let ffmpeg_available = check_ffmpeg_available();
    let pdftoppm_available = check_pdftoppm_available();

    Json(FormatsResponse {
        // Image: usa image crate (pure Rust, sempre disponibile)
        image: FormatSupport {
            input: formats::IMAGE_INPUT.iter().map(|s| s.to_string()).collect(),
            output: formats::IMAGE_OUTPUT.iter().map(|s| s.to_string()).collect(),
            available: true,
        },
        // SVG: usa resvg crate (pure Rust, sempre disponibile)
        svg: FormatSupport {
            input: formats::SVG_INPUT.iter().map(|s| s.to_string()).collect(),
            output: formats::SVG_OUTPUT.iter().map(|s| s.to_string()).collect(),
            available: true,
        },
        // Document: usa printpdf crate (pure Rust, sempre disponibile)
        document: FormatSupport {
            input: formats::DOCUMENT_INPUT.iter().map(|s| s.to_string()).collect(),
            output: formats::DOCUMENT_OUTPUT.iter().map(|s| s.to_string()).collect(),
            available: true,
        },
        // Audio: richiede FFmpeg
        audio: FormatSupport {
            input: if ffmpeg_available {
                formats::AUDIO_INPUT.iter().map(|s| s.to_string()).collect()
            } else {
                vec![]
            },
            output: if ffmpeg_available {
                formats::AUDIO_OUTPUT.iter().map(|s| s.to_string()).collect()
            } else {
                vec![]
            },
            available: ffmpeg_available,
        },
        // Video: richiede FFmpeg
        video: FormatSupport {
            input: if ffmpeg_available {
                formats::VIDEO_INPUT.iter().map(|s| s.to_string()).collect()
            } else {
                vec![]
            },
            output: if ffmpeg_available {
                formats::VIDEO_OUTPUT.iter().map(|s| s.to_string()).collect()
            } else {
                vec![]
            },
            available: ffmpeg_available,
        },
        // PDF: richiede pdftoppm (poppler-utils)
        pdf: FormatSupport {
            input: if pdftoppm_available {
                formats::PDF_INPUT.iter().map(|s| s.to_string()).collect()
            } else {
                vec![]
            },
            output: if pdftoppm_available {
                formats::PDF_OUTPUT.iter().map(|s| s.to_string()).collect()
            } else {
                vec![]
            },
            available: pdftoppm_available,
        },
        max_file_size_mb: state.max_file_size_mb,
    })
}
