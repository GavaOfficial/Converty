use axum::{
    extract::{ConnectInfo, Multipart, Query, State},
    http::{header, HeaderMap},
    response::IntoResponse,
    routing::post,
    Extension, Json, Router,
};
use std::net::SocketAddr;
use std::time::Instant;

use crate::db::api_keys::ApiKeyRole;
use crate::db::stats::{self, ConversionRecordDb, GuestConfig};
use crate::db::DbPool;
use crate::error::{AppError, Result};
use crate::handlers::image as image_handler;
use crate::handlers::pdf as pdf_handler;
use crate::models::{
    BatchConvertResponse, ConversionType, ConvertQuery, ConvertedFile, FailedFile, ImageOptions,
    PdfConvertQuery,
};
use crate::services::{converter, queue::JobQueue};
use crate::utils::get_extension;

#[derive(Clone)]
pub struct ConvertState {
    pub job_queue: JobQueue,
    pub db: DbPool,
}

pub fn router(job_queue: JobQueue, db: DbPool) -> Router {
    let state = ConvertState { job_queue, db };
    Router::new()
        .route("/api/v1/convert/image", post(convert_image))
        .route("/api/v1/convert/document", post(convert_document))
        .route("/api/v1/convert/audio", post(convert_audio))
        .route("/api/v1/convert/video", post(convert_video))
        .route("/api/v1/convert/pdf", post(convert_pdf))
        .route("/api/v1/convert/batch", post(convert_batch))
        .with_state(state)
}

/// Info utente autenticato
#[derive(Clone, Debug)]
pub struct AuthInfo {
    pub api_key_id: Option<String>,
    pub is_guest: bool,
    pub role: ApiKeyRole,
    pub client_ip: Option<String>,
}

/// Converti un'immagine
#[utoipa::path(
    post,
    path = "/api/v1/convert/image",
    params(
        ("output_format" = String, Query, description = "Formato output: png, jpg, webp, gif, bmp"),
        ("quality" = Option<u8>, Query, description = "Qualità (1-100)"),
        ("width" = Option<u32>, Query, description = "Larghezza in pixel"),
        ("height" = Option<u32>, Query, description = "Altezza in pixel"),
    ),
    responses(
        (status = 200, description = "File convertito", content_type = "application/octet-stream"),
        (status = 400, description = "Formato non supportato"),
        (status = 401, description = "API Key non valida"),
        (status = 429, description = "Troppe richieste o limite giornaliero"),
    ),
    security(("api_key" = [])),
    tag = "Conversione"
)]
pub async fn convert_image(
    State(state): State<ConvertState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<ConvertQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    let start = Instant::now();

    // Verifica limiti guest
    if auth.is_guest {
        check_guest_limits(&state.db, &auth, "image").await?;
    }

    // Estrai file dal multipart
    let field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::MissingField("file".to_string()))?;

    let filename = field.file_name().unwrap_or("file").to_string();
    let input_format = get_extension(&filename).unwrap_or_default();
    let data = field
        .bytes()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let input_size = data.len() as i64;

    // Verifica dimensione file per guest
    if auth.is_guest {
        check_guest_file_size(&state.db, input_size).await?;
    }

    // Crea opzioni immagine con resize
    let options = ImageOptions::from_query(&query);

    // Esegui conversione con resize
    let result = image_handler::convert_image(&data, &input_format, &query.output_format, &options);

    match result {
        Ok(output) => {
            let output_size = output.len() as i64;

            // Registra conversione nel database
            record_conversion(
                &state.db,
                &auth,
                "image",
                &input_format,
                &query.output_format,
                input_size,
                output_size,
                start.elapsed().as_millis() as i64,
                true,
                None,
            )
            .await;

            // Incrementa uso guest
            if auth.is_guest {
                if let Some(ip) = &auth.client_ip {
                    let _ = stats::increment_guest_usage(&state.db, ip).await;
                }
            }

            let content_type = get_content_type(&query.output_format);
            let output_filename = format!(
                "{}.{}",
                filename
                    .rsplit_once('.')
                    .map(|(n, _)| n)
                    .unwrap_or(&filename),
                query.output_format
            );

            Ok((
                [
                    (header::CONTENT_TYPE, content_type),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", output_filename),
                    ),
                ],
                output,
            ))
        }
        Err(e) => {
            // Registra errore
            record_conversion(
                &state.db,
                &auth,
                "image",
                &input_format,
                &query.output_format,
                input_size,
                0,
                start.elapsed().as_millis() as i64,
                false,
                Some(e.to_string()),
            )
            .await;

            Err(e)
        }
    }
}

/// Converti un documento
#[utoipa::path(
    post,
    path = "/api/v1/convert/document",
    params(
        ("output_format" = String, Query, description = "Formato output: pdf, txt, html"),
    ),
    responses(
        (status = 200, description = "File convertito", content_type = "application/octet-stream"),
        (status = 400, description = "Formato non supportato"),
    ),
    security(("api_key" = [])),
    tag = "Conversione"
)]
pub async fn convert_document(
    State(state): State<ConvertState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<ConvertQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    convert_single_tracked(
        &state,
        &auth,
        &mut multipart,
        &query,
        ConversionType::Document,
    )
    .await
}

/// Converti un file audio (richiede FFmpeg)
#[utoipa::path(
    post,
    path = "/api/v1/convert/audio",
    params(
        ("output_format" = String, Query, description = "Formato output: mp3, wav, ogg, flac"),
        ("quality" = Option<u8>, Query, description = "Qualità (1-100)"),
    ),
    responses(
        (status = 200, description = "File convertito", content_type = "application/octet-stream"),
        (status = 400, description = "Formato non supportato"),
        (status = 503, description = "FFmpeg non disponibile"),
    ),
    security(("api_key" = [])),
    tag = "Conversione"
)]
pub async fn convert_audio(
    State(state): State<ConvertState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<ConvertQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    convert_single_tracked(&state, &auth, &mut multipart, &query, ConversionType::Audio).await
}

/// Converti un file video (richiede FFmpeg)
#[utoipa::path(
    post,
    path = "/api/v1/convert/video",
    params(
        ("output_format" = String, Query, description = "Formato output: mp4, webm, avi, gif"),
        ("quality" = Option<u8>, Query, description = "Qualità (1-100)"),
    ),
    responses(
        (status = 200, description = "File convertito", content_type = "application/octet-stream"),
        (status = 400, description = "Formato non supportato"),
        (status = 503, description = "FFmpeg non disponibile"),
    ),
    security(("api_key" = [])),
    tag = "Conversione"
)]
pub async fn convert_video(
    State(state): State<ConvertState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<ConvertQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    convert_single_tracked(&state, &auth, &mut multipart, &query, ConversionType::Video).await
}

/// Converti un PDF in immagine (richiede pdftoppm/poppler)
#[utoipa::path(
    post,
    path = "/api/v1/convert/pdf",
    params(
        ("output_format" = String, Query, description = "Formato output: png, jpg, tiff"),
        ("page" = Option<u32>, Query, description = "Numero pagina (default: 1)"),
        ("dpi" = Option<u32>, Query, description = "Risoluzione DPI (default: 150)"),
        ("all_pages" = Option<bool>, Query, description = "Converti tutte le pagine in ZIP (default: false)"),
    ),
    responses(
        (status = 200, description = "File convertito (immagine singola o ZIP con tutte le pagine)", content_type = "application/octet-stream"),
        (status = 400, description = "Formato non supportato"),
        (status = 503, description = "pdftoppm non disponibile"),
    ),
    security(("api_key" = [])),
    tag = "Conversione"
)]
pub async fn convert_pdf(
    State(state): State<ConvertState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<PdfConvertQuery>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    let start = Instant::now();

    // Verifica limiti guest
    if auth.is_guest {
        check_guest_limits(&state.db, &auth, "pdf").await?;
    }

    // Estrai file dal multipart
    let field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::MissingField("file".to_string()))?;

    let filename = field.file_name().unwrap_or("file.pdf").to_string();
    let base_name = filename
        .rsplit_once('.')
        .map(|(n, _)| n)
        .unwrap_or(&filename);
    let data = field
        .bytes()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let input_size = data.len() as i64;

    // Verifica dimensione file per guest
    if auth.is_guest {
        check_guest_file_size(&state.db, input_size).await?;
    }

    // Esegui conversione PDF -> Immagine (singola o tutte le pagine)
    let result = if query.all_pages {
        // Converti tutte le pagine e crea ZIP
        pdf_handler::convert_pdf_to_zip(&data, &query.output_format, Some(query.dpi), base_name)
    } else {
        // Converti singola pagina
        pdf_handler::convert_pdf_to_image(
            &data,
            &query.output_format,
            Some(query.page),
            Some(query.dpi),
        )
    };

    match result {
        Ok(output) => {
            let output_size = output.len() as i64;
            let output_format_for_stats = if query.all_pages {
                "zip"
            } else {
                &query.output_format
            };

            // Registra conversione nel database
            record_conversion(
                &state.db,
                &auth,
                "pdf",
                "pdf",
                output_format_for_stats,
                input_size,
                output_size,
                start.elapsed().as_millis() as i64,
                true,
                None,
            )
            .await;

            // Incrementa uso guest
            if auth.is_guest {
                if let Some(ip) = &auth.client_ip {
                    let _ = stats::increment_guest_usage(&state.db, ip).await;
                }
            }

            // Determina content type e nome file in base al tipo di output
            let (content_type, output_filename) = if query.all_pages {
                (
                    "application/zip".to_string(),
                    format!("{}_pages.zip", base_name),
                )
            } else {
                (
                    get_content_type(&query.output_format),
                    format!("{}_page{}.{}", base_name, query.page, query.output_format),
                )
            };

            Ok((
                [
                    (header::CONTENT_TYPE, content_type),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", output_filename),
                    ),
                ],
                output,
            ))
        }
        Err(e) => {
            // Registra errore
            record_conversion(
                &state.db,
                &auth,
                "pdf",
                "pdf",
                &query.output_format,
                input_size,
                0,
                start.elapsed().as_millis() as i64,
                false,
                Some(e.to_string()),
            )
            .await;

            Err(e)
        }
    }
}

async fn convert_single_tracked(
    state: &ConvertState,
    auth: &AuthInfo,
    multipart: &mut Multipart,
    query: &ConvertQuery,
    conversion_type: ConversionType,
) -> Result<impl IntoResponse> {
    let start = Instant::now();
    let type_str = conversion_type.to_string();

    // Verifica limiti guest
    if auth.is_guest {
        check_guest_limits(&state.db, auth, &type_str).await?;
    }

    // Estrai file dal multipart
    let field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::MissingField("file".to_string()))?;

    let filename = field.file_name().unwrap_or("file").to_string();
    let input_format = get_extension(&filename).unwrap_or_default();
    let data = field
        .bytes()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let input_size = data.len() as i64;

    // Verifica dimensione file per guest
    if auth.is_guest {
        check_guest_file_size(&state.db, input_size).await?;
    }

    // Esegui conversione
    let result = converter::convert(
        &data,
        &input_format,
        &query.output_format,
        &conversion_type,
        query.quality,
    );

    match result {
        Ok(output) => {
            let output_size = output.len() as i64;

            // Registra conversione
            record_conversion(
                &state.db,
                auth,
                &type_str,
                &input_format,
                &query.output_format,
                input_size,
                output_size,
                start.elapsed().as_millis() as i64,
                true,
                None,
            )
            .await;

            // Incrementa uso guest
            if auth.is_guest {
                if let Some(ip) = &auth.client_ip {
                    let _ = stats::increment_guest_usage(&state.db, ip).await;
                }
            }

            let content_type = get_content_type(&query.output_format);
            let output_filename = format!(
                "{}.{}",
                filename
                    .rsplit_once('.')
                    .map(|(n, _)| n)
                    .unwrap_or(&filename),
                query.output_format
            );

            Ok((
                [
                    (header::CONTENT_TYPE, content_type),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", output_filename),
                    ),
                ],
                output,
            ))
        }
        Err(e) => {
            // Registra errore
            record_conversion(
                &state.db,
                auth,
                &type_str,
                &input_format,
                &query.output_format,
                input_size,
                0,
                start.elapsed().as_millis() as i64,
                false,
                Some(e.to_string()),
            )
            .await;

            Err(e)
        }
    }
}

/// Converti multipli file in batch
#[utoipa::path(
    post,
    path = "/api/v1/convert/batch",
    params(
        ("output_format" = String, Query, description = "Formato output"),
        ("quality" = Option<u8>, Query, description = "Qualità (1-100)"),
    ),
    responses(
        (status = 200, description = "Risultato batch", body = BatchConvertResponse),
    ),
    security(("api_key" = [])),
    tag = "Conversione"
)]
pub async fn convert_batch(
    State(state): State<ConvertState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<ConvertQuery>,
    mut multipart: Multipart,
) -> Result<Json<BatchConvertResponse>> {
    // Guest non può usare batch
    if auth.is_guest {
        return Err(AppError::Forbidden(
            "Batch conversion non disponibile per utenti guest".to_string(),
        ));
    }

    let mut converted = Vec::new();
    let mut failed = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    {
        let start = Instant::now();
        let filename = field.file_name().unwrap_or("file").to_string();
        let input_format = get_extension(&filename).unwrap_or_default();

        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => {
                failed.push(FailedFile {
                    original_name: filename,
                    error: e.to_string(),
                });
                continue;
            }
        };

        let input_size = data.len() as i64;

        // Determina tipo conversione automaticamente
        let conversion_type = converter::detect_conversion_type(&input_format);

        if let Some(conv_type) = conversion_type {
            let type_str = conv_type.to_string();

            match converter::convert(
                &data,
                &input_format,
                &query.output_format,
                &conv_type,
                query.quality,
            ) {
                Ok(output) => {
                    let output_size = output.len() as i64;

                    // Registra successo
                    record_conversion(
                        &state.db,
                        &auth,
                        &type_str,
                        &input_format,
                        &query.output_format,
                        input_size,
                        output_size,
                        start.elapsed().as_millis() as i64,
                        true,
                        None,
                    )
                    .await;

                    converted.push(ConvertedFile {
                        original_name: filename,
                        output_format: query.output_format.clone(),
                        size_bytes: output_size as u64,
                    });
                }
                Err(e) => {
                    // Registra errore
                    record_conversion(
                        &state.db,
                        &auth,
                        &type_str,
                        &input_format,
                        &query.output_format,
                        input_size,
                        0,
                        start.elapsed().as_millis() as i64,
                        false,
                        Some(e.to_string()),
                    )
                    .await;

                    failed.push(FailedFile {
                        original_name: filename,
                        error: e.to_string(),
                    });
                }
            }
        } else {
            failed.push(FailedFile {
                original_name: filename,
                error: format!("Formato non supportato: {}", input_format),
            });
        }
    }

    Ok(Json(BatchConvertResponse {
        success: failed.is_empty(),
        converted,
        failed,
    }))
}

async fn record_conversion(
    db: &DbPool,
    auth: &AuthInfo,
    conversion_type: &str,
    input_format: &str,
    output_format: &str,
    input_size: i64,
    output_size: i64,
    processing_time_ms: i64,
    success: bool,
    error: Option<String>,
) {
    let record = ConversionRecordDb {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        api_key_id: auth.api_key_id.clone(),
        is_guest: auth.is_guest,
        conversion_type: conversion_type.to_string(),
        input_format: input_format.to_string(),
        output_format: output_format.to_string(),
        input_size_bytes: input_size,
        output_size_bytes: output_size,
        processing_time_ms,
        success,
        error,
        client_ip: auth.client_ip.clone(),
    };

    if let Err(e) = stats::insert_conversion(db, &record).await {
        tracing::error!("Errore salvataggio statistiche: {}", e);
    }
}

async fn check_guest_limits(db: &DbPool, auth: &AuthInfo, conversion_type: &str) -> Result<()> {
    let config = stats::get_guest_config(db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !config.enabled {
        return Err(AppError::Forbidden(
            "Modalità guest disabilitata. Richiedi una API Key.".to_string(),
        ));
    }

    // Verifica tipo conversione permesso
    if !config.allowed_types.iter().any(|t| t == conversion_type) {
        return Err(AppError::Forbidden(format!(
            "Tipo conversione '{}' non permesso per guest. Tipi permessi: {}",
            conversion_type,
            config.allowed_types.join(", ")
        )));
    }

    // Verifica limite giornaliero
    if let Some(ip) = &auth.client_ip {
        let daily_usage = stats::get_guest_daily_usage(db, ip)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        if daily_usage >= config.daily_limit {
            return Err(AppError::DailyLimitExceeded(format!(
                "Limite giornaliero di {} conversioni raggiunto per guest",
                config.daily_limit
            )));
        }
    }

    Ok(())
}

async fn check_guest_file_size(db: &DbPool, size_bytes: i64) -> Result<()> {
    let config = stats::get_guest_config(db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let max_bytes = config.max_file_size_mb * 1024 * 1024;
    if size_bytes > max_bytes {
        return Err(AppError::FileTooLarge(config.max_file_size_mb as u64));
    }

    Ok(())
}

fn get_content_type(format: &str) -> String {
    match format.to_lowercase().as_str() {
        // Immagini
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        // Documenti
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "html" => "text/html",
        // Audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        // Video
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        _ => "application/octet-stream",
    }
    .to_string()
}
