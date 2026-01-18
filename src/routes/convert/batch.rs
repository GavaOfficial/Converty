//! Batch conversion endpoint

use axum::{extract::Multipart, extract::Query, extract::State, Extension, Json};
use std::time::Instant;

use crate::error::{AppError, Result};
use crate::models::{AuthInfo, BatchConvertResponse, ConvertQuery, ConvertedFile, FailedFile};
use crate::services::converter;
use crate::utils::get_extension;

use super::helpers::record_conversion;
use super::ConvertState;

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
