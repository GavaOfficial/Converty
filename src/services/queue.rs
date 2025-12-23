//! Queue service per gestione job con persistenza database

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Semaphore};
use uuid::Uuid;

use crate::db::jobs::JobRecord;
use crate::db::{jobs as db_jobs, oauth_users, user_settings, DbPool};
use crate::error::{AppError, Result};
use crate::models::{ConversionType, Job, JobStatus, ProgressUpdate};
use crate::services::converter;
use crate::services::google_drive::GoogleDriveService;

/// Capacità del broadcast channel per progress updates
const PROGRESS_CHANNEL_CAPACITY: usize = 100;

/// Numero massimo di job concorrenti globali
const MAX_CONCURRENT_JOBS: usize = 10;

pub type JobQueue = Arc<RwLock<JobQueueInner>>;

/// Sender globale per progress updates
pub type ProgressSender = broadcast::Sender<ProgressUpdate>;

pub fn create_job_queue(db: DbPool) -> (JobQueue, ProgressSender) {
    let (tx, _) = broadcast::channel(PROGRESS_CHANNEL_CAPACITY);
    let queue = Arc::new(RwLock::new(JobQueueInner::new(tx.clone(), db)));
    (queue, tx)
}

pub struct JobQueueInner {
    temp_dir: PathBuf,
    progress_tx: ProgressSender,
    db: DbPool,
    concurrency_semaphore: Arc<Semaphore>,
}

impl std::fmt::Debug for JobQueueInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobQueueInner")
            .field("temp_dir", &self.temp_dir)
            .finish()
    }
}

impl JobQueueInner {
    pub fn new(progress_tx: ProgressSender, db: DbPool) -> Self {
        let temp_dir = std::env::temp_dir().join("converty").join("jobs");
        std::fs::create_dir_all(&temp_dir).ok();

        Self {
            temp_dir,
            progress_tx,
            db,
            concurrency_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS)),
        }
    }

    /// Invia un progress update via broadcast
    pub fn send_progress(&self, update: ProgressUpdate) {
        // Ignora errore se nessun receiver (nessun client connesso)
        let _ = self.progress_tx.send(update);
    }

    /// Ottieni un receiver per ricevere progress updates
    pub fn subscribe(&self) -> broadcast::Receiver<ProgressUpdate> {
        self.progress_tx.subscribe()
    }

    /// Ottieni riferimento al database
    pub fn db(&self) -> &DbPool {
        &self.db
    }

    /// Ottieni semaforo per concorrenza
    pub fn semaphore(&self) -> Arc<Semaphore> {
        self.concurrency_semaphore.clone()
    }

    pub async fn create_job(
        &self,
        conversion_type: ConversionType,
        input_data: Vec<u8>,
        input_format: String,
        output_format: String,
        quality: Option<u8>,
        api_key_id: Option<String>,
        priority: Option<String>,
        webhook_url: Option<String>,
        source_url: Option<String>,
        expires_in_hours: Option<i64>,
        original_filename: Option<String>,
    ) -> Result<Uuid> {
        // Controlla limite job per utente se autenticato
        if let Some(ref key_id) = api_key_id {
            let user_active = db_jobs::count_user_active_jobs(&self.db, key_id)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            let user_limit = db_jobs::get_user_job_limit(&self.db, key_id)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            if user_active >= user_limit {
                return Err(AppError::TooManyJobs(format!(
                    "Limite job raggiunto: {}/{}",
                    user_active, user_limit
                )));
            }
        }

        // I job vengono sempre accettati e messi in coda.
        // Il semaforo in process_job controlla la concorrenza effettiva.

        // Salva input in file temporaneo
        let job_id = Uuid::new_v4();
        let job_dir = self.temp_dir.join(job_id.to_string());
        std::fs::create_dir_all(&job_dir)?;

        let input_path = job_dir.join(format!("input.{}", input_format));
        let file_size = input_data.len() as i64;
        std::fs::write(&input_path, input_data)?;

        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();

        // Calcola data di scadenza
        let expires_at =
            expires_in_hours.map(|hours| (now + chrono::Duration::hours(hours)).to_rfc3339());

        // Crea record nel database
        let job_record = JobRecord {
            id: job_id.to_string(),
            api_key_id,
            conversion_type: conversion_type.to_string(),
            input_format: input_format.clone(),
            output_format: output_format.clone(),
            quality: quality.map(|q| q as i64),
            status: "pending".to_string(),
            progress: 0,
            progress_message: None,
            input_path: input_path.to_string_lossy().to_string(),
            result_path: None,
            error: None,
            file_size_bytes: Some(file_size),
            created_at: now_str.clone(),
            started_at: None,
            completed_at: None,
            updated_at: now_str,
            priority: priority.or(Some("normal".to_string())),
            webhook_url,
            source_url,
            expires_at,
            retry_count: Some(0),
            original_filename,
            drive_file_id: None,
        };

        db_jobs::create_job(&self.db, &job_record)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        // Invia progress iniziale
        let update = ProgressUpdate::new(job_id, JobStatus::Pending, 0, None);
        self.send_progress(update);

        Ok(job_id)
    }

    pub async fn get_job(&self, id: &Uuid) -> Result<Option<Job>> {
        let record = db_jobs::get_job(&self.db, &id.to_string())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(record.map(|r| job_from_record(&r)))
    }

    pub async fn delete_job(&self, id: &Uuid) -> Result<()> {
        // Ottieni job per pulire i file
        let record = db_jobs::get_job(&self.db, &id.to_string())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        if let Some(job) = record {
            // Rimuovi file temporanei
            let job_dir = self.temp_dir.join(id.to_string());
            std::fs::remove_dir_all(job_dir).ok();

            if let Some(result_path) = job.result_path {
                std::fs::remove_file(result_path).ok();
            }

            // Elimina dal database
            db_jobs::delete_job(&self.db, &id.to_string())
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            Ok(())
        } else {
            Err(AppError::JobNotFound(id.to_string()))
        }
    }

    /// Aggiorna progress di un job e invia notifica
    pub async fn update_job_progress(&self, id: &Uuid, progress: u8, message: Option<String>) {
        let msg_ref = message.as_deref();
        let _ = db_jobs::update_job_status(
            &self.db,
            &id.to_string(),
            "processing",
            progress as i64,
            msg_ref,
            None,
            None,
        )
        .await;

        let update = ProgressUpdate::new(*id, JobStatus::Processing, progress, message);
        self.send_progress(update);
    }

    /// Marca job come processing e invia notifica
    pub async fn mark_job_processing(&self, id: &Uuid) {
        let _ = db_jobs::update_job_status(
            &self.db,
            &id.to_string(),
            "processing",
            0,
            Some("Avvio conversione..."),
            None,
            None,
        )
        .await;

        let update = ProgressUpdate::new(
            *id,
            JobStatus::Processing,
            0,
            Some("Avvio conversione...".to_string()),
        );
        self.send_progress(update);
    }

    /// Marca job come completato e invia notifica
    pub async fn mark_job_completed(&self, id: &Uuid, result_path: PathBuf) {
        let result_path_str = result_path.to_string_lossy().to_string();
        let _ = db_jobs::update_job_status(
            &self.db,
            &id.to_string(),
            "completed",
            100,
            Some("Conversione completata!"),
            None,
            Some(&result_path_str),
        )
        .await;

        let update = ProgressUpdate::new(
            *id,
            JobStatus::Completed,
            100,
            Some("Conversione completata!".to_string()),
        );
        self.send_progress(update);
    }

    /// Marca job come fallito e invia notifica
    pub async fn mark_job_failed(&self, id: &Uuid, error: String) {
        let _ = db_jobs::update_job_status(
            &self.db,
            &id.to_string(),
            "failed",
            0,
            Some(&format!("Errore: {}", error)),
            Some(&error),
            None,
        )
        .await;

        let update = ProgressUpdate::new(
            *id,
            JobStatus::Failed,
            0,
            Some(format!("Errore: {}", error)),
        );
        self.send_progress(update);
    }
}

/// Converte un JobRecord dal database in un Job
fn job_from_record(r: &JobRecord) -> Job {
    let status = match r.status.as_str() {
        "pending" => JobStatus::Pending,
        "processing" => JobStatus::Processing,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        _ => JobStatus::Pending,
    };

    let conversion_type = match r.conversion_type.as_str() {
        "image" => ConversionType::Image,
        "document" => ConversionType::Document,
        "audio" => ConversionType::Audio,
        "video" => ConversionType::Video,
        _ => ConversionType::Image,
    };

    Job {
        id: Uuid::parse_str(&r.id).unwrap_or_else(|_| Uuid::new_v4()),
        status,
        conversion_type,
        input_path: PathBuf::from(&r.input_path),
        input_format: r.input_format.clone(),
        output_format: r.output_format.clone(),
        quality: r.quality.map(|q| q as u8),
        created_at: chrono::DateTime::parse_from_rfc3339(&r.created_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        completed_at: r.completed_at.as_ref().and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok()
        }),
        result_path: r.result_path.as_ref().map(PathBuf::from),
        error: r.error.clone(),
        progress: r.progress as u8,
        progress_message: r.progress_message.clone(),
    }
}

pub async fn process_job(queue: JobQueue, job_id: Uuid) {
    // Acquisisci permesso dal semaforo
    let semaphore = {
        let q = queue.read().await;
        q.semaphore()
    };

    let _permit = match semaphore.acquire().await {
        Ok(p) => p,
        Err(_) => return,
    };

    // Marca come in elaborazione
    {
        let q = queue.read().await;
        q.mark_job_processing(&job_id).await;
    }

    // Leggi dati job dal database (incluso api_key_id e original_filename per Drive)
    let (job, api_key_id, original_filename) = {
        let q = queue.read().await;
        match q.get_job(&job_id).await {
            Ok(Some(job)) => {
                // Get the full job record for api_key_id and original_filename
                let record = db_jobs::get_job(q.db(), &job_id.to_string())
                    .await
                    .ok()
                    .flatten();
                let api_key_id = record.as_ref().and_then(|r| r.api_key_id.clone());
                let original_filename = record.as_ref().and_then(|r| r.original_filename.clone());
                (job, api_key_id, original_filename)
            }
            _ => return,
        }
    };

    let input_path = job.input_path;
    let output_format = job.output_format.clone();
    let conversion_type = job.conversion_type;
    let quality = job.quality;

    // Progress: caricamento file
    {
        let q = queue.read().await;
        q.update_job_progress(&job_id, 10, Some("Caricamento file...".to_string()))
            .await;
    }

    // Esegui conversione
    let temp_dir = std::env::temp_dir()
        .join("converty")
        .join("jobs")
        .join(job_id.to_string());
    // Assicura che la directory esista
    std::fs::create_dir_all(&temp_dir).ok();

    // Progress: conversione in corso
    {
        let q = queue.read().await;
        q.update_job_progress(&job_id, 30, Some("Conversione in corso...".to_string()))
            .await;
    }

    // Gestione speciale per PDF multi-pagina
    let is_pdf = matches!(conversion_type, ConversionType::Pdf)
        || input_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            == Some("pdf".to_string());

    let (result, actual_output_path) = if is_pdf {
        match converter::convert_pdf_file_smart(&input_path, &temp_dir, &output_format) {
            Ok((path, _is_zip)) => (Ok(()), path),
            Err(e) => (Err(e), temp_dir.join(format!("output.{}", output_format))),
        }
    } else {
        let output_path = temp_dir.join(format!("output.{}", output_format));
        let res = converter::convert_file(
            &input_path,
            &output_path,
            &output_format,
            &conversion_type,
            quality,
        );
        (res, output_path)
    };

    // Progress: salvataggio
    {
        let q = queue.read().await;
        q.update_job_progress(&job_id, 80, Some("Salvataggio risultato...".to_string()))
            .await;
    }

    // Aggiorna stato job
    let (final_status, error_msg, completed_output_path) = {
        let q = queue.read().await;
        match result {
            Ok(_) => {
                q.mark_job_completed(&job_id, actual_output_path.clone())
                    .await;
                ("completed", None, Some(actual_output_path))
            }
            Err(e) => {
                let err = e.to_string();
                q.mark_job_failed(&job_id, err.clone()).await;
                ("failed", Some(err), None)
            }
        }
    };

    // Upload to Google Drive if enabled (only for completed jobs)
    if final_status == "completed" {
        if let (Some(key_id), Some(result_path)) = (&api_key_id, &completed_output_path) {
            let q = queue.read().await;
            let db = q.db().clone();
            let job_id_str = job_id.to_string();
            let key_id = key_id.clone();
            let result_path = result_path.clone();
            let original_filename = original_filename.clone();
            let output_format = output_format.clone();
            let conv_type_str = conversion_type.to_string();

            // Get Google credentials from env
            let google_client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
            let google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();

            if !google_client_id.is_empty() && !google_client_secret.is_empty() {
                tokio::spawn(async move {
                    upload_to_drive_if_enabled(
                        &db,
                        &job_id_str,
                        &key_id,
                        &result_path,
                        original_filename.as_deref(),
                        &output_format,
                        &conv_type_str,
                        &google_client_id,
                        &google_client_secret,
                    )
                    .await;
                });
            }
        }
    }

    // Invia webhook se configurato
    {
        let q = queue.read().await;
        if let Ok(Some(webhook_url)) = db_jobs::get_job_webhook(q.db(), &job_id.to_string()).await {
            let error_clone = error_msg.clone();
            tokio::spawn(async move {
                send_webhook(&webhook_url, &job_id, final_status, error_clone.as_deref()).await;
            });
        }
    }
}

pub async fn get_job_result(queue: &JobQueue, job_id: &Uuid) -> Result<Vec<u8>> {
    let q = queue.read().await;

    let job = q
        .get_job(job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.to_string()))?;

    if job.status != JobStatus::Completed {
        return Err(AppError::JobNotCompleted);
    }

    let result_path = job
        .result_path
        .as_ref()
        .ok_or_else(|| AppError::Internal("Percorso risultato mancante".to_string()))?;

    let data = std::fs::read(result_path)?;

    Ok(data)
}

/// Scarica un file da URL remoto
pub async fn download_from_url(url: &str) -> Result<(Vec<u8>, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| AppError::Internal(format!("Errore client HTTP: {}", e)))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Errore download URL: {}", e)))?;

    if !response.status().is_success() {
        return Err(AppError::Internal(format!(
            "Errore HTTP {}: impossibile scaricare il file",
            response.status()
        )));
    }

    // Estrai estensione dall'URL o dal content-type
    let extension = extract_extension_from_url(url)
        .or_else(|| {
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .and_then(extension_from_mime)
        })
        .unwrap_or_else(|| "bin".to_string());

    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("Errore lettura response: {}", e)))?;

    Ok((bytes.to_vec(), extension))
}

/// Estrae l'estensione del file dall'URL
fn extract_extension_from_url(url: &str) -> Option<String> {
    url.rsplit('/')
        .next()
        .and_then(|filename| filename.split('?').next())
        .and_then(|filename| {
            filename
                .rsplit('.')
                .next()
                .filter(|ext| ext.len() <= 5 && ext.chars().all(|c| c.is_alphanumeric()))
                .map(|s| s.to_lowercase())
        })
}

/// Converte MIME type in estensione
fn extension_from_mime(mime: &str) -> Option<String> {
    match mime.split(';').next().unwrap_or("").trim() {
        "image/png" => Some("png".to_string()),
        "image/jpeg" => Some("jpg".to_string()),
        "image/gif" => Some("gif".to_string()),
        "image/webp" => Some("webp".to_string()),
        "image/svg+xml" => Some("svg".to_string()),
        "application/pdf" => Some("pdf".to_string()),
        "text/plain" => Some("txt".to_string()),
        "text/html" => Some("html".to_string()),
        "audio/mpeg" => Some("mp3".to_string()),
        "audio/wav" => Some("wav".to_string()),
        "video/mp4" => Some("mp4".to_string()),
        "video/webm" => Some("webm".to_string()),
        _ => None,
    }
}

/// Invia notifica webhook
pub async fn send_webhook(webhook_url: &str, job_id: &Uuid, status: &str, error: Option<&str>) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Errore creazione client webhook: {}", e);
            return;
        }
    };

    let payload = serde_json::json!({
        "job_id": job_id.to_string(),
        "status": status,
        "error": error,
        "timestamp": chrono::Utc::now().to_rfc3339()
    });

    match client.post(webhook_url).json(&payload).send().await {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!("Webhook inviato con successo per job {}", job_id);
            } else {
                tracing::warn!(
                    "Webhook per job {} ha ritornato status {}",
                    job_id,
                    response.status()
                );
            }
        }
        Err(e) => {
            tracing::error!("Errore invio webhook per job {}: {}", job_id, e);
        }
    }
}

/// Upload file to Google Drive if enabled for user
pub async fn upload_to_drive_if_enabled(
    db: &DbPool,
    job_id: &str,
    api_key_id: &str,
    result_path: &PathBuf,
    original_filename: Option<&str>,
    output_format: &str,
    conversion_type: &str,
    google_client_id: &str,
    google_client_secret: &str,
) {
    // Find user by API key
    let user_id = match oauth_users::get_user_id_by_api_key(db, api_key_id).await {
        Ok(Some(id)) => id,
        _ => {
            tracing::debug!("No OAuth user found for api_key_id: {}", api_key_id);
            return;
        }
    };

    // Check if Drive is enabled for user
    let settings = match user_settings::get_settings(db, &user_id).await {
        Ok(Some(s)) if s.save_to_drive_enabled => s,
        _ => {
            tracing::debug!("Drive not enabled for user: {}", user_id);
            return;
        }
    };

    // Check if this conversion type should be saved to Drive
    if !user_settings::should_save_to_drive(&settings.drive_filter_types, conversion_type) {
        tracing::debug!(
            "Conversion type '{}' not in Drive filter '{}' for user: {}",
            conversion_type,
            settings.drive_filter_types,
            user_id
        );
        return;
    }

    // Create Drive service and get valid token
    let drive = GoogleDriveService::new();
    let access_token = match drive
        .get_valid_token(db, &user_id, google_client_id, google_client_secret)
        .await
    {
        Ok(token) => token,
        Err(e) => {
            tracing::error!("Failed to get Drive token for user {}: {}", user_id, e);
            return;
        }
    };

    // Ensure folder exists
    let folder_id = match drive
        .ensure_folder(&access_token, &settings.drive_folder_name)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Failed to ensure Drive folder: {}", e);
            return;
        }
    };

    // Determine filename
    let filename = if settings.auto_save_original_filename {
        original_filename
            .map(|name| {
                // Replace extension with output format
                let base = name.rsplit_once('.').map(|(base, _)| base).unwrap_or(name);
                format!("{}.{}", base, output_format)
            })
            .unwrap_or_else(|| format!("converted.{}", output_format))
    } else {
        format!(
            "converted_{}.{}",
            chrono::Utc::now().format("%Y%m%d_%H%M%S"),
            output_format
        )
    };

    // Upload file
    match drive
        .upload_file_from_path(&access_token, &folder_id, result_path, Some(&filename))
        .await
    {
        Ok(file) => {
            tracing::info!("File uploaded to Drive: {} (id: {})", file.name, file.id);
            // Save drive_file_id to job record
            if let Err(e) = db_jobs::update_job_drive_file_id(db, job_id, &file.id).await {
                tracing::error!("Failed to save drive_file_id for job {}: {}", job_id, e);
            }
        }
        Err(e) => {
            tracing::error!("Failed to upload to Drive: {}", e);
        }
    }
}
