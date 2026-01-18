//! Conversion routes module
//!
//! This module provides HTTP endpoints for file conversion operations.

mod batch;
mod endpoints;
mod guest;
mod helpers;

use axum::{routing::post, Router};

use crate::db::DbPool;
use crate::services::queue::JobQueue;

// Re-export AuthInfo for backwards compatibility
pub use crate::models::AuthInfo;

// Re-export public items (including utoipa path types)
pub use batch::*;
pub use endpoints::*;
pub use guest::{check_guest_file_size, check_guest_limits};

/// Shared state for conversion routes
#[derive(Clone)]
pub struct ConvertState {
    pub job_queue: JobQueue,
    pub db: DbPool,
}

/// Create the router for conversion endpoints
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
