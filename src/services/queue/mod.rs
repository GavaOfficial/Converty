//! Job queue service module
//!
//! This module provides asynchronous job processing with database persistence.

mod core;
mod processor;
mod webhooks;

// Re-export public items
pub use core::{create_job_queue, job_from_record, JobQueue, JobQueueInner, ProgressSender};
pub use processor::{download_from_url, get_job_result, process_job};
pub use webhooks::send_webhook;

#[cfg(feature = "google-auth")]
pub use webhooks::upload_to_drive_if_enabled;
