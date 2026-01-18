//! Job management routes module
//!
//! This module provides HTTP endpoints for managing asynchronous conversion jobs.

mod crud;
#[cfg(feature = "google-auth")]
mod drive;
mod stream;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::db::DbPool;
use crate::services::queue::{JobQueue, ProgressSender};

// Re-export public items (including utoipa path types)
pub use crud::*;
#[cfg(feature = "google-auth")]
pub use drive::*;
pub use stream::*;

/// Shared state for job routes
#[derive(Clone)]
pub struct JobsState {
    pub queue: JobQueue,
    pub progress_tx: ProgressSender,
    pub db: DbPool,
}

/// Create the router for job endpoints (with google-auth feature)
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

/// Create the router for job endpoints (without google-auth feature)
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
