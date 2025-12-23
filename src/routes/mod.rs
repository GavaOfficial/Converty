pub mod admin;
#[cfg(feature = "google-auth")]
pub mod auth;
pub mod convert;
pub mod health;
pub mod jobs;
pub mod settings;
pub mod stats;

use axum::Router;

use crate::config::Config;
use crate::db::DbPool;
use crate::services::queue::{JobQueue, ProgressSender};

#[cfg(feature = "google-auth")]
pub fn create_router(
    job_queue: JobQueue,
    progress_tx: ProgressSender,
    db: DbPool,
    config: Config,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    frontend_url: String,
) -> Router {
    Router::new()
        .merge(health::router(config.max_file_size_mb))
        .merge(convert::router(job_queue.clone(), db.clone()))
        .merge(jobs::router(job_queue, progress_tx, db.clone()))
        .merge(stats::router(db.clone()))
        .merge(admin::router(db.clone()))
        .merge(settings::router(db.clone()))
        .merge(auth::router(db, google_client_id, google_client_secret, frontend_url))
}

#[cfg(not(feature = "google-auth"))]
pub fn create_router(
    job_queue: JobQueue,
    progress_tx: ProgressSender,
    db: DbPool,
    config: Config,
    _google_client_id: Option<String>,
    _google_client_secret: Option<String>,
    _frontend_url: String,
) -> Router {
    Router::new()
        .merge(health::router(config.max_file_size_mb))
        .merge(convert::router(job_queue.clone(), db.clone()))
        .merge(jobs::router(job_queue, progress_tx, db.clone()))
        .merge(stats::router(db.clone()))
        .merge(admin::router(db.clone()))
        .merge(settings::router(db.clone()))
}
