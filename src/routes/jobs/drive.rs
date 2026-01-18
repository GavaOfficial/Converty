//! Google Drive integration for jobs (feature-gated)

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    Extension, Json,
};

use crate::db::jobs as db_jobs;
use crate::db::oauth_users;
use crate::error::{AppError, Result};
use crate::models::AuthInfo;
use crate::services::google_drive::GoogleDriveService;

use super::JobsState;

/// Parametri query per thumbnail
#[derive(Debug, serde::Deserialize)]
pub struct ThumbnailQuery {
    /// Dimensione della thumbnail (default 80)
    #[serde(default = "default_thumbnail_size")]
    pub size: u32,
}

fn default_thumbnail_size() -> u32 {
    80
}

/// Elimina un file da Google Drive
#[utoipa::path(
    delete,
    path = "/api/v1/jobs/{id}/drive",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "File eliminato da Drive"),
        (status = 400, description = "Il job non ha un file su Drive"),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn delete_drive_file(
    State(state): State<JobsState>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // Verifica autenticazione
    let api_key_id = auth
        .api_key_id
        .ok_or_else(|| AppError::Unauthorized("Autenticazione richiesta".to_string()))?;

    // Verifica che il job esista e appartenga all'utente
    let job = db_jobs::get_job(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::JobNotFound(id.clone()))?;

    // Verifica che il job appartenga all'utente
    if job.api_key_id.as_ref() != Some(&api_key_id) {
        return Err(AppError::Unauthorized("Non autorizzato".to_string()));
    }

    // Verifica che ci sia un drive_file_id
    let drive_file_id = job
        .drive_file_id
        .ok_or_else(|| AppError::BadRequest("Il job non ha un file su Google Drive".to_string()))?;

    // Trova l'utente OAuth associato all'API key
    let user_id = oauth_users::get_user_id_by_api_key(&state.db, &api_key_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorized("Utente OAuth non trovato".to_string()))?;

    // Ottieni credenziali Google
    let google_client_id = std::env::var("GOOGLE_CLIENT_ID")
        .map_err(|_| AppError::Internal("GOOGLE_CLIENT_ID non configurato".to_string()))?;
    let google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET")
        .map_err(|_| AppError::Internal("GOOGLE_CLIENT_SECRET non configurato".to_string()))?;

    // Ottieni token valido
    let drive = GoogleDriveService::new();
    let access_token = drive
        .get_valid_token(
            &state.db,
            &user_id,
            &google_client_id,
            &google_client_secret,
        )
        .await
        .map_err(|e| AppError::Internal(format!("Impossibile ottenere token: {}", e)))?;

    // Elimina il file da Drive
    drive
        .delete_file(&access_token, &drive_file_id)
        .await
        .map_err(|e| AppError::Internal(format!("Errore eliminazione file: {}", e)))?;

    // Rimuovi drive_file_id dal job
    db_jobs::clear_job_drive_file_id(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "File eliminato da Google Drive"
    })))
}

/// Ottieni la thumbnail di un file su Google Drive
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/thumbnail",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job"),
        ("size" = Option<u32>, Query, description = "Dimensione thumbnail (default 80)")
    ),
    responses(
        (status = 200, description = "Thumbnail image"),
        (status = 400, description = "Il job non ha un file su Drive"),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn get_drive_thumbnail(
    State(state): State<JobsState>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
    Query(query): Query<ThumbnailQuery>,
) -> Result<impl IntoResponse> {
    // Verifica autenticazione
    let api_key_id = auth
        .api_key_id
        .ok_or_else(|| AppError::Unauthorized("Autenticazione richiesta".to_string()))?;

    // Verifica che il job esista e appartenga all'utente
    let job = db_jobs::get_job(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::JobNotFound(id.clone()))?;

    // Verifica che il job appartenga all'utente
    if job.api_key_id.as_ref() != Some(&api_key_id) {
        return Err(AppError::Unauthorized("Non autorizzato".to_string()));
    }

    // Verifica che ci sia un drive_file_id
    let drive_file_id = job
        .drive_file_id
        .ok_or_else(|| AppError::BadRequest("Il job non ha un file su Google Drive".to_string()))?;

    // Trova l'utente OAuth associato all'API key
    let user_id = oauth_users::get_user_id_by_api_key(&state.db, &api_key_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorized("Utente OAuth non trovato".to_string()))?;

    // Ottieni credenziali Google
    let google_client_id = std::env::var("GOOGLE_CLIENT_ID")
        .map_err(|_| AppError::Internal("GOOGLE_CLIENT_ID non configurato".to_string()))?;
    let google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET")
        .map_err(|_| AppError::Internal("GOOGLE_CLIENT_SECRET non configurato".to_string()))?;

    // Ottieni token valido
    let drive = GoogleDriveService::new();
    let access_token = drive
        .get_valid_token(
            &state.db,
            &user_id,
            &google_client_id,
            &google_client_secret,
        )
        .await
        .map_err(|e| AppError::Internal(format!("Impossibile ottenere token: {}", e)))?;

    // Ottieni la thumbnail
    let thumbnail_data = drive
        .get_thumbnail(&access_token, &drive_file_id, query.size)
        .await
        .map_err(|e| AppError::Internal(format!("Errore thumbnail: {}", e)))?;

    // Ritorna l'immagine con cache headers
    Ok((
        [
            (header::CONTENT_TYPE, "image/png".to_string()),
            (header::CACHE_CONTROL, "public, max-age=3600".to_string()),
        ],
        thumbnail_data,
    ))
}
