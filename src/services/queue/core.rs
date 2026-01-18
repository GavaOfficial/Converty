//! Core job queue structures and basic operations

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Semaphore};
use uuid::Uuid;

use crate::db::jobs::JobRecord;
use crate::db::{jobs as db_jobs, DbPool};
use crate::error::{AppError, Result};
use crate::models::{ConversionType, Job, JobStatus, ProgressUpdate};

/// Capacità del broadcast channel per progress updates
const PROGRESS_CHANNEL_CAPACITY: usize = 100;

/// Numero massimo di job concorrenti globali
const MAX_CONCURRENT_JOBS: usize = 10;

/// Type alias for the job queue
pub type JobQueue = Arc<RwLock<JobQueueInner>>;

/// Sender globale per progress updates
pub type ProgressSender = broadcast::Sender<ProgressUpdate>;

/// Create a new job queue with database pool
pub fn create_job_queue(db: DbPool) -> (JobQueue, ProgressSender) {
    let (tx, _) = broadcast::channel(PROGRESS_CHANNEL_CAPACITY);
    let queue = Arc::new(RwLock::new(JobQueueInner::new(tx.clone(), db)));
    (queue, tx)
}

/// Inner job queue structure
pub struct JobQueueInner {
    pub(crate) temp_dir: PathBuf,
    pub(crate) progress_tx: ProgressSender,
    pub(crate) db: DbPool,
    pub(crate) concurrency_semaphore: Arc<Semaphore>,
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

    #[allow(clippy::too_many_arguments)]
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
pub fn job_from_record(r: &JobRecord) -> Job {
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
