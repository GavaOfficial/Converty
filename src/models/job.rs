use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use utoipa::ToSchema;
use uuid::Uuid;

use super::ConversionType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Processing => write!(f, "processing"),
            JobStatus::Completed => write!(f, "completed"),
            JobStatus::Failed => write!(f, "failed"),
            JobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Aggiornamento progress per SSE streaming
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProgressUpdate {
    #[schema(value_type = String)]
    pub job_id: Uuid,
    pub status: JobStatus,
    pub progress: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[schema(value_type = String)]
    pub timestamp: DateTime<Utc>,
}

impl ProgressUpdate {
    pub fn new(job_id: Uuid, status: JobStatus, progress: u8, message: Option<String>) -> Self {
        Self {
            job_id,
            status,
            progress,
            message,
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub status: JobStatus,
    pub conversion_type: ConversionType,
    pub input_path: PathBuf,
    pub input_format: String,
    pub output_format: String,
    pub quality: Option<u8>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_path: Option<PathBuf>,
    pub error: Option<String>,
    pub progress: u8,
    pub progress_message: Option<String>,
}

impl Job {
    pub fn new(
        conversion_type: ConversionType,
        input_path: PathBuf,
        input_format: String,
        output_format: String,
        quality: Option<u8>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: JobStatus::Pending,
            conversion_type,
            input_path,
            input_format,
            output_format,
            quality,
            created_at: Utc::now(),
            completed_at: None,
            result_path: None,
            error: None,
            progress: 0,
            progress_message: None,
        }
    }

    pub fn mark_processing(&mut self) {
        self.status = JobStatus::Processing;
        self.progress = 0;
        self.progress_message = Some("Avvio conversione...".to_string());
    }

    pub fn update_progress(&mut self, progress: u8, message: Option<String>) {
        self.progress = progress.min(100);
        self.progress_message = message;
    }

    pub fn mark_completed(&mut self, result_path: PathBuf) {
        self.status = JobStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.result_path = Some(result_path);
        self.progress = 100;
        self.progress_message = Some("Conversione completata!".to_string());
    }

    pub fn mark_failed(&mut self, error: String) {
        self.status = JobStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error.clone());
        self.progress_message = Some(format!("Errore: {}", error));
    }

    /// Crea un ProgressUpdate dal job corrente
    pub fn to_progress_update(&self) -> ProgressUpdate {
        ProgressUpdate::new(
            self.id,
            self.status.clone(),
            self.progress,
            self.progress_message.clone(),
        )
    }
}
