//! Webhook and external service integration

use uuid::Uuid;

#[cfg(feature = "google-auth")]
use crate::db::DbPool;

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
#[cfg(feature = "google-auth")]
#[allow(clippy::too_many_arguments)]
pub async fn upload_to_drive_if_enabled(
    db: &DbPool,
    job_id: &str,
    api_key_id: &str,
    result_path: &std::path::Path,
    original_filename: Option<&str>,
    output_format: &str,
    conversion_type: &str,
    google_client_id: &str,
    google_client_secret: &str,
) {
    use crate::db::{jobs as db_jobs, oauth_users, user_settings};
    use crate::services::google_drive::GoogleDriveService;

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
