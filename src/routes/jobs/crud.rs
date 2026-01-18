//! CRUD operations for jobs

use axum::{
    extract::{Multipart, Path, Query, State},
    http::header,
    response::IntoResponse,
    Extension, Json,
};
use uuid::Uuid;

use crate::db::api_keys::ApiKeyRole;
use crate::db::jobs::{self as db_jobs, JobsListResponse, JobsQuery};
use crate::db::stats;
use crate::error::{AppError, Result};
use crate::models::{
    AuthInfo, CreateJobRequest, JobCreatedResponse, JobResponse, JobStatus, ProgressUpdate,
};
use crate::services::queue::{self, download_from_url};
use crate::utils::{get_content_type, get_extension};

use super::JobsState;

/// Response per history
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct HistoryResponse {
    pub jobs: Vec<stats::ConversionHistoryItem>,
}

/// Query per history
#[derive(Debug, serde::Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_limit")]
    pub limit: i64,
    /// Filtro data: today, week, month, all
    #[serde(default)]
    pub date_filter: Option<String>,
    /// Filtro formato input
    #[serde(default)]
    pub input_format: Option<String>,
    /// Filtro formato output
    #[serde(default)]
    pub output_format: Option<String>,
    /// Filtro stato: completed, failed, all
    #[serde(default)]
    pub status: Option<String>,
}

fn default_history_limit() -> i64 {
    50
}

/// Lista tutti i job con paginazione e filtri
#[utoipa::path(
    get,
    path = "/api/v1/jobs",
    tag = "Jobs",
    params(
        ("status" = Option<String>, Query, description = "Filtra per stato (pending, processing, completed, failed)"),
        ("conversion_type" = Option<String>, Query, description = "Filtra per tipo conversione"),
        ("limit" = Option<i64>, Query, description = "Limite risultati (default 50)"),
        ("offset" = Option<i64>, Query, description = "Offset per paginazione"),
    ),
    responses(
        (status = 200, description = "Lista job", body = JobsListResponse),
    )
)]
pub async fn list_jobs(
    State(state): State<JobsState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<JobsQuery>,
) -> Result<Json<JobsListResponse>> {
    // Solo admin può vedere tutti i job
    if auth.role != ApiKeyRole::Admin {
        return Err(AppError::Forbidden(
            "Solo gli admin possono vedere la lista completa dei job".to_string(),
        ));
    }

    let response = db_jobs::list_jobs(&state.db, &query)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(response))
}

/// Ottieni la cronologia delle conversioni dell'utente
#[utoipa::path(
    get,
    path = "/api/v1/jobs/history",
    tag = "Jobs",
    params(
        ("limit" = Option<i64>, Query, description = "Limite risultati (default 50)"),
        ("date_filter" = Option<String>, Query, description = "Filtro data: today, week, month, all"),
        ("input_format" = Option<String>, Query, description = "Filtro formato input"),
        ("output_format" = Option<String>, Query, description = "Filtro formato output"),
        ("status" = Option<String>, Query, description = "Filtro stato: completed, failed, all"),
    ),
    responses(
        (status = 200, description = "Cronologia conversioni", body = HistoryResponse),
        (status = 401, description = "API Key richiesta"),
    )
)]
pub async fn get_history(
    State(state): State<JobsState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>> {
    // Richiede autenticazione
    let api_key_id = auth.api_key_id.ok_or_else(|| {
        AppError::Unauthorized("API Key richiesta per vedere la cronologia".to_string())
    })?;

    // Costruisci filtri
    let filters = stats::HistoryFilters {
        date_filter: query.date_filter,
        input_format: query.input_format,
        output_format: query.output_format,
        status: query.status,
    };

    let jobs =
        stats::get_user_conversions_filtered(&state.db, &api_key_id, query.limit, Some(&filters))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(HistoryResponse { jobs }))
}

/// Crea un nuovo job di conversione asincrono
#[utoipa::path(
    post,
    path = "/api/v1/jobs",
    tag = "Jobs",
    request_body(content_type = "multipart/form-data"),
    params(
        ("output_format" = String, Query, description = "Formato di output"),
        ("conversion_type" = String, Query, description = "Tipo conversione: image, document, audio, video"),
        ("quality" = Option<u8>, Query, description = "Qualità (1-100)"),
        ("source_url" = Option<String>, Query, description = "URL sorgente (alternativa a upload file)"),
        ("priority" = Option<String>, Query, description = "Priorità: low, normal, high"),
        ("webhook_url" = Option<String>, Query, description = "URL webhook per notifica completamento"),
        ("expires_in_hours" = Option<i64>, Query, description = "Ore prima della scadenza risultato")
    ),
    responses(
        (status = 200, description = "Job creato", body = JobCreatedResponse),
        (status = 400, description = "Richiesta non valida"),
        (status = 429, description = "Troppi job in coda"),
    )
)]
pub async fn create_job(
    State(state): State<JobsState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<CreateJobRequest>,
    mut multipart: Multipart,
) -> Result<Json<JobCreatedResponse>> {
    // Determina sorgente dati: URL o upload
    let (data, input_format, original_filename) = if let Some(ref source_url) = query.source_url {
        // Scarica da URL - estrai filename dall'URL
        let (bytes, ext) = download_from_url(source_url).await?;
        let url_filename = source_url
            .rsplit('/')
            .next()
            .and_then(|s| s.split('?').next())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        (bytes, ext, url_filename)
    } else {
        // Estrai file da multipart
        let field = multipart
            .next_field()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
            .ok_or_else(|| AppError::MissingField("file o source_url".to_string()))?;

        let filename = field.file_name().unwrap_or("file").to_string();
        let input_format = get_extension(&filename).unwrap_or_default();
        let original_filename = if filename != "file" {
            Some(filename.clone())
        } else {
            None
        };
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        (bytes.to_vec(), input_format, original_filename)
    };

    // Crea job con nuovi parametri
    let job_id = {
        let q = state.queue.read().await;
        q.create_job(
            query.conversion_type.clone(),
            data,
            input_format,
            query.output_format.clone(),
            query.quality,
            auth.api_key_id,
            Some(query.priority.to_string()),
            query.webhook_url.clone(),
            query.source_url.clone(),
            query.expires_in_hours,
            original_filename,
        )
        .await?
    };

    // Avvia elaborazione in background
    let queue_clone = state.queue.clone();
    tokio::spawn(async move {
        queue::process_job(queue_clone, job_id).await;
    });

    Ok(Json(JobCreatedResponse {
        id: job_id.to_string(),
        message: "Job creato e in elaborazione".to_string(),
    }))
}

/// Ottiene lo stato di un job
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "Stato del job", body = JobResponse),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn get_job_status(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<Json<JobResponse>> {
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;

    let q = state.queue.read().await;
    let job = q
        .get_job(&job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(id))?;

    Ok(Json(JobResponse {
        id: job.id.to_string(),
        status: job.status.clone(),
        conversion_type: job.conversion_type.to_string(),
        input_format: job.input_format.clone(),
        output_format: job.output_format.clone(),
        created_at: job.created_at.to_rfc3339(),
        completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
        error: job.error.clone(),
    }))
}

/// Elimina un job e i suoi file temporanei
#[utoipa::path(
    delete,
    path = "/api/v1/jobs/{id}",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "Job eliminato"),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn delete_job(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;

    let q = state.queue.read().await;
    q.delete_job(&job_id).await?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Job eliminato"
    })))
}

/// Scarica il risultato di un job completato
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/download",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "File convertito"),
        (status = 404, description = "Job non trovato"),
        (status = 202, description = "Job non ancora completato"),
    )
)]
pub async fn download_job_result(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;

    // Ottieni job info incluso result_path
    let (output_format, result_path) = {
        let q = state.queue.read().await;
        let job = q
            .get_job(&job_id)
            .await?
            .ok_or_else(|| AppError::JobNotFound(id.clone()))?;
        (job.output_format.clone(), job.result_path.clone())
    };

    // Ottieni risultato
    let data = queue::get_job_result(&state.queue, &job_id).await?;

    // Determina il tipo effettivo del file dal path del risultato
    let actual_extension = result_path
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or(&output_format);

    // Usa l'estensione reale per content-type e filename
    let content_type = get_content_type(actual_extension).to_string();
    let filename = format!("converted.{}", actual_extension);

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        data,
    ))
}

/// Riprova un job fallito
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/retry",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "Job rimesso in coda"),
        (status = 400, description = "Il job non è in stato failed"),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn retry_job(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // Verifica che il job esista e sia in stato failed
    let job = db_jobs::get_job(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::JobNotFound(id.clone()))?;

    if job.status != "failed" {
        return Err(AppError::BadRequest(
            "Solo i job falliti possono essere ritentati".to_string(),
        ));
    }

    // Controlla il numero di retry
    let retry_count = job.retry_count.unwrap_or(0);
    const MAX_RETRIES: i64 = 3;
    if retry_count >= MAX_RETRIES {
        return Err(AppError::BadRequest(format!(
            "Numero massimo di retry raggiunto ({}/{})",
            retry_count, MAX_RETRIES
        )));
    }

    // Reset del job per retry
    let success = db_jobs::reset_job_for_retry(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !success {
        return Err(AppError::Internal(
            "Impossibile resettare il job".to_string(),
        ));
    }

    // Avvia elaborazione in background
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;
    let queue_clone = state.queue.clone();
    tokio::spawn(async move {
        queue::process_job(queue_clone, job_id).await;
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Job rimesso in coda (retry {}/{})", retry_count + 1, MAX_RETRIES),
        "retry_count": retry_count + 1
    })))
}

/// Cancella un job in corso o in attesa
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/cancel",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "Job cancellato"),
        (status = 400, description = "Il job non può essere cancellato"),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn cancel_job(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // Verifica che il job esista
    let job = db_jobs::get_job(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::JobNotFound(id.clone()))?;

    // Solo pending e processing possono essere cancellati
    if job.status != "pending" && job.status != "processing" {
        return Err(AppError::BadRequest(format!(
            "Il job con stato '{}' non può essere cancellato",
            job.status
        )));
    }

    // Cancella il job
    let success = db_jobs::cancel_job(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !success {
        return Err(AppError::Internal(
            "Impossibile cancellare il job".to_string(),
        ));
    }

    // Invia notifica di cancellazione via SSE
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;
    let update = ProgressUpdate::new(
        job_id,
        JobStatus::Cancelled,
        0,
        Some("Job cancellato dall'utente".to_string()),
    );
    let _ = state.progress_tx.send(update);

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Job cancellato"
    })))
}
