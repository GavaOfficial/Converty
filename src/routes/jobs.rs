//! Routes per gestione job asincroni

use axum::{
    extract::{Multipart, Path, Query, State},
    http::header,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post},
    Extension, Json, Router,
};
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::Stream;
use uuid::Uuid;

use crate::db::api_keys::ApiKeyRole;
use crate::db::jobs::{self as db_jobs, JobsListResponse, JobsQuery};
use crate::db::stats;
use crate::db::DbPool;
use crate::error::{AppError, Result};
use crate::models::{CreateJobRequest, JobCreatedResponse, JobResponse, JobStatus, ProgressUpdate};
use crate::routes::convert::AuthInfo;
use crate::services::queue::{self, download_from_url, JobQueue, ProgressSender};
use crate::utils::get_extension;

/// Stato condiviso per le routes dei jobs
#[derive(Clone)]
pub struct JobsState {
    pub queue: JobQueue,
    pub progress_tx: ProgressSender,
    pub db: DbPool,
}

#[cfg(feature = "google-auth")]
pub fn router(job_queue: JobQueue, progress_tx: ProgressSender, db: DbPool) -> Router {
    let state = JobsState {
        queue: job_queue,
        progress_tx,
        db,
    };

    Router::new()
        .route("/api/v1/jobs", get(list_jobs))
        .route("/api/v1/jobs", post(create_job))
        .route("/api/v1/jobs/history", get(get_history))
        .route("/api/v1/jobs/:id", get(get_job_status))
        .route("/api/v1/jobs/:id", delete(delete_job))
        .route("/api/v1/jobs/:id/download", get(download_job_result))
        .route("/api/v1/jobs/:id/progress", get(job_progress_stream))
        .route("/api/v1/jobs/:id/retry", post(retry_job))
        .route("/api/v1/jobs/:id/cancel", post(cancel_job))
        .route("/api/v1/jobs/:id/drive", delete(delete_drive_file))
        .route("/api/v1/jobs/:id/thumbnail", get(get_drive_thumbnail))
        .with_state(state)
}

#[cfg(not(feature = "google-auth"))]
pub fn router(job_queue: JobQueue, progress_tx: ProgressSender, db: DbPool) -> Router {
    let state = JobsState {
        queue: job_queue,
        progress_tx,
        db,
    };

    Router::new()
        .route("/api/v1/jobs", get(list_jobs))
        .route("/api/v1/jobs", post(create_job))
        .route("/api/v1/jobs/history", get(get_history))
        .route("/api/v1/jobs/:id", get(get_job_status))
        .route("/api/v1/jobs/:id", delete(delete_job))
        .route("/api/v1/jobs/:id/download", get(download_job_result))
        .route("/api/v1/jobs/:id/progress", get(job_progress_stream))
        .route("/api/v1/jobs/:id/retry", post(retry_job))
        .route("/api/v1/jobs/:id/cancel", post(cancel_job))
        .with_state(state)
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
    let content_type = get_content_type(actual_extension);
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

/// Stream personalizzato per progress di un job
struct JobProgressStream {
    job_id: Uuid,
    rx: BroadcastStream<ProgressUpdate>,
    initial_sent: bool,
    initial_update: Option<ProgressUpdate>,
    terminated: bool,
}

impl Stream for JobProgressStream {
    type Item = std::result::Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Se già terminato, non produrre più eventi
        if self.terminated {
            return Poll::Ready(None);
        }

        // Prima invia l'evento iniziale
        if !self.initial_sent {
            self.initial_sent = true;
            if let Some(update) = self.initial_update.take() {
                let json = serde_json::to_string(&update).unwrap_or_default();
                // Controlla se già terminale
                if update.status == JobStatus::Completed || update.status == JobStatus::Failed {
                    self.terminated = true;
                }
                return Poll::Ready(Some(Ok(Event::default().data(json))));
            }
        }

        // Poi ascolta nuovi eventi dal broadcast
        let rx = Pin::new(&mut self.rx);
        match rx.poll_next(cx) {
            Poll::Ready(Some(Ok(update))) => {
                if update.job_id == self.job_id {
                    let json = serde_json::to_string(&update).unwrap_or_default();
                    // Controlla se terminale
                    if update.status == JobStatus::Completed || update.status == JobStatus::Failed {
                        self.terminated = true;
                    }
                    Poll::Ready(Some(Ok(Event::default().data(json))))
                } else {
                    // Non è il nostro job, continua a pollare
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Poll::Ready(Some(Err(_))) => {
                // Errore broadcast (lag), continua
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Stream SSE per monitorare il progress di un job in tempo reale
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/progress",
    tag = "Jobs",
    params(
        ("id" = String, Path, description = "ID del job")
    ),
    responses(
        (status = 200, description = "Stream SSE con aggiornamenti progress", body = ProgressUpdate),
        (status = 404, description = "Job non trovato"),
    )
)]
pub async fn job_progress_stream(
    State(state): State<JobsState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    let job_id = Uuid::parse_str(&id).map_err(|_| AppError::JobNotFound(id.clone()))?;

    // Verifica che il job esista e ottieni stato iniziale
    let initial_update = {
        let q = state.queue.read().await;
        let job = q
            .get_job(&job_id)
            .await?
            .ok_or_else(|| AppError::JobNotFound(id.clone()))?;
        job.to_progress_update()
    };

    // Subscribe al broadcast channel
    let rx = state.progress_tx.subscribe();

    let stream = JobProgressStream {
        job_id,
        rx: BroadcastStream::new(rx),
        initial_sent: false,
        initial_update: Some(initial_update),
        terminated: false,
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn get_content_type(format: &str) -> String {
    match format.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        "html" => "text/html",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_string()
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

/// Elimina un file da Google Drive
#[cfg(feature = "google-auth")]
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
    use crate::db::oauth_users;
    use crate::services::google_drive::GoogleDriveService;

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

/// Parametri query per thumbnail
#[cfg(feature = "google-auth")]
#[derive(Debug, serde::Deserialize)]
pub struct ThumbnailQuery {
    /// Dimensione della thumbnail (default 80)
    #[serde(default = "default_thumbnail_size")]
    pub size: u32,
}

#[cfg(feature = "google-auth")]
fn default_thumbnail_size() -> u32 {
    80
}

/// Ottieni la thumbnail di un file su Google Drive
#[cfg(feature = "google-auth")]
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
    use crate::db::oauth_users;
    use crate::services::google_drive::GoogleDriveService;

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
