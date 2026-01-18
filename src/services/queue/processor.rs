//! Job processing logic

use uuid::Uuid;

use crate::db::jobs as db_jobs;
use crate::error::{AppError, Result};
use crate::models::{ConversionType, JobStatus};
use crate::services::converter;

use super::core::JobQueue;
use super::webhooks::send_webhook;

/// Process a job
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
    #[allow(unused_variables)]
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
    #[allow(unused_variables)]
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
    #[cfg(feature = "google-auth")]
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
                    super::webhooks::upload_to_drive_if_enabled(
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

/// Get the result of a completed job
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
