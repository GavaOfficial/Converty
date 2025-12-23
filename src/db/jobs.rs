//! Modulo per la gestione dei job nel database

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use super::DbPool;

/// Record job nel database
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct JobRecord {
    pub id: String,
    pub api_key_id: Option<String>,
    pub conversion_type: String,
    pub input_format: String,
    pub output_format: String,
    pub quality: Option<i64>,
    pub status: String,
    pub progress: i64,
    pub progress_message: Option<String>,
    pub input_path: String,
    pub result_path: Option<String>,
    pub error: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
    // Nuovi campi
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub retry_count: Option<i64>,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub drive_file_id: Option<String>,
}

/// Query per lista job
#[derive(Debug, Deserialize, ToSchema)]
pub struct JobsQuery {
    pub status: Option<String>,
    pub conversion_type: Option<String>,
    pub api_key_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Response lista job
#[derive(Debug, Serialize, ToSchema)]
pub struct JobsListResponse {
    pub jobs: Vec<JobRecord>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Crea un nuovo job nel database
pub async fn create_job(pool: &DbPool, job: &JobRecord) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO jobs (
            id, api_key_id, conversion_type, input_format, output_format,
            quality, status, progress, progress_message, input_path,
            result_path, error, file_size_bytes, created_at, started_at,
            completed_at, updated_at, priority, webhook_url, source_url,
            expires_at, retry_count, original_filename, drive_file_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&job.id)
    .bind(&job.api_key_id)
    .bind(&job.conversion_type)
    .bind(&job.input_format)
    .bind(&job.output_format)
    .bind(job.quality)
    .bind(&job.status)
    .bind(job.progress)
    .bind(&job.progress_message)
    .bind(&job.input_path)
    .bind(&job.result_path)
    .bind(&job.error)
    .bind(job.file_size_bytes)
    .bind(&job.created_at)
    .bind(&job.started_at)
    .bind(&job.completed_at)
    .bind(&job.updated_at)
    .bind(&job.priority)
    .bind(&job.webhook_url)
    .bind(&job.source_url)
    .bind(&job.expires_at)
    .bind(job.retry_count)
    .bind(&job.original_filename)
    .bind(&job.drive_file_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Ottieni un job per ID
pub async fn get_job(pool: &DbPool, id: &str) -> Result<Option<JobRecord>, sqlx::Error> {
    sqlx::query_as::<_, JobRecord>(
        r#"
        SELECT id, api_key_id, conversion_type, input_format, output_format,
               quality, status, progress, progress_message, input_path,
               result_path, error, file_size_bytes, created_at, started_at,
               completed_at, updated_at, priority, webhook_url, source_url,
               expires_at, retry_count, original_filename, drive_file_id
        FROM jobs WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Lista job con filtri e paginazione
pub async fn list_jobs(pool: &DbPool, query: &JobsQuery) -> Result<JobsListResponse, sqlx::Error> {
    // Query per il conteggio totale
    let mut count_sql = String::from("SELECT COUNT(*) FROM jobs WHERE 1=1");
    let mut params: Vec<String> = Vec::new();

    if let Some(status) = &query.status {
        count_sql.push_str(" AND status = ?");
        params.push(status.clone());
    }
    if let Some(conv_type) = &query.conversion_type {
        count_sql.push_str(" AND conversion_type = ?");
        params.push(conv_type.clone());
    }
    if let Some(api_key) = &query.api_key_id {
        count_sql.push_str(" AND api_key_id = ?");
        params.push(api_key.clone());
    }

    // Esegui count
    let total: (i64,) = {
        let mut q = sqlx::query_as(&count_sql);
        for p in &params {
            q = q.bind(p);
        }
        q.fetch_one(pool).await?
    };

    // Query per i dati
    let mut data_sql = String::from(
        r#"
        SELECT id, api_key_id, conversion_type, input_format, output_format,
               quality, status, progress, progress_message, input_path,
               result_path, error, file_size_bytes, created_at, started_at,
               completed_at, updated_at, priority, webhook_url, source_url,
               expires_at, retry_count, original_filename, drive_file_id
        FROM jobs WHERE 1=1
        "#,
    );

    if query.status.is_some() {
        data_sql.push_str(" AND status = ?");
    }
    if query.conversion_type.is_some() {
        data_sql.push_str(" AND conversion_type = ?");
    }
    if query.api_key_id.is_some() {
        data_sql.push_str(" AND api_key_id = ?");
    }

    data_sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

    let jobs: Vec<JobRecord> = {
        let mut q = sqlx::query_as::<_, JobRecord>(&data_sql);
        for p in &params {
            q = q.bind(p);
        }
        q = q.bind(query.limit).bind(query.offset);
        q.fetch_all(pool).await?
    };

    Ok(JobsListResponse {
        jobs,
        total: total.0,
        limit: query.limit,
        offset: query.offset,
    })
}

/// Aggiorna lo stato di un job
pub async fn update_job_status(
    pool: &DbPool,
    id: &str,
    status: &str,
    progress: i64,
    progress_message: Option<&str>,
    error: Option<&str>,
    result_path: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();

    let mut sql = String::from(
        "UPDATE jobs SET status = ?, progress = ?, progress_message = ?, updated_at = ?",
    );

    if error.is_some() {
        sql.push_str(", error = ?");
    }
    if result_path.is_some() {
        sql.push_str(", result_path = ?");
    }
    if status == "processing" {
        sql.push_str(", started_at = ?");
    }
    if status == "completed" || status == "failed" {
        sql.push_str(", completed_at = ?");
    }

    sql.push_str(" WHERE id = ?");

    let mut query = sqlx::query(&sql)
        .bind(status)
        .bind(progress)
        .bind(progress_message)
        .bind(&now);

    if let Some(e) = error {
        query = query.bind(e);
    }
    if let Some(rp) = result_path {
        query = query.bind(rp);
    }
    if status == "processing" {
        query = query.bind(&now);
    }
    if status == "completed" || status == "failed" {
        query = query.bind(&now);
    }

    query = query.bind(id);

    let result = query.execute(pool).await?;
    Ok(result.rows_affected() > 0)
}

/// Elimina un job
pub async fn delete_job(pool: &DbPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM jobs WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Conta i job attivi (pending o processing)
pub async fn count_active_jobs(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM jobs WHERE status IN ('pending', 'processing')",
    )
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Conta i job attivi per un utente specifico
pub async fn count_user_active_jobs(pool: &DbPool, api_key_id: &str) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM jobs WHERE api_key_id = ? AND status IN ('pending', 'processing')",
    )
    .bind(api_key_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Ottieni il limite di job concorrenti per un'API key
pub async fn get_user_job_limit(pool: &DbPool, api_key_id: &str) -> Result<i64, sqlx::Error> {
    let row: (Option<i64>,) = sqlx::query_as(
        "SELECT max_concurrent_jobs FROM api_keys WHERE id = ?",
    )
    .bind(api_key_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0.unwrap_or(5))
}

/// Pulisci job vecchi (completati o falliti) più vecchi di N giorni
pub async fn cleanup_old_jobs(pool: &DbPool, days: i64) -> Result<(u64, Vec<String>), sqlx::Error> {
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();

    // Prima ottieni i path dei file da eliminare
    let paths: Vec<(Option<String>, String)> = sqlx::query_as(
        r#"
        SELECT result_path, input_path FROM jobs
        WHERE status IN ('completed', 'failed')
        AND created_at < ?
        "#,
    )
    .bind(&cutoff)
    .fetch_all(pool)
    .await?;

    let mut files_to_delete: Vec<String> = Vec::new();
    for (result_path, input_path) in paths {
        files_to_delete.push(input_path);
        if let Some(rp) = result_path {
            files_to_delete.push(rp);
        }
    }

    // Elimina i record dal database
    let result = sqlx::query(
        r#"
        DELETE FROM jobs
        WHERE status IN ('completed', 'failed')
        AND created_at < ?
        "#,
    )
    .bind(&cutoff)
    .execute(pool)
    .await?;

    Ok((result.rows_affected(), files_to_delete))
}

/// Ottieni job in timeout (processing da troppo tempo)
pub async fn get_timed_out_jobs(pool: &DbPool, timeout_seconds: i64) -> Result<Vec<String>, sqlx::Error> {
    let cutoff = (Utc::now() - Duration::seconds(timeout_seconds)).to_rfc3339();

    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT id FROM jobs
        WHERE status = 'processing'
        AND started_at < ?
        "#,
    )
    .bind(&cutoff)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Marca job come fallito per timeout
pub async fn mark_job_timed_out(pool: &DbPool, id: &str) -> Result<bool, sqlx::Error> {
    update_job_status(
        pool,
        id,
        "failed",
        0,
        Some("Job timeout"),
        Some("Il job ha superato il tempo massimo di esecuzione"),
        None,
    )
    .await
}

/// Resetta un job fallito per ritentare
pub async fn reset_job_for_retry(pool: &DbPool, id: &str) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        r#"
        UPDATE jobs SET
            status = 'pending',
            progress = 0,
            progress_message = 'In coda per retry...',
            error = NULL,
            started_at = NULL,
            completed_at = NULL,
            retry_count = COALESCE(retry_count, 0) + 1,
            updated_at = ?
        WHERE id = ? AND status = 'failed'
        "#,
    )
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Marca un job come cancellato
pub async fn cancel_job(pool: &DbPool, id: &str) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        r#"
        UPDATE jobs SET
            status = 'cancelled',
            progress_message = 'Job cancellato dall''utente',
            completed_at = ?,
            updated_at = ?
        WHERE id = ? AND status IN ('pending', 'processing')
        "#,
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Ottieni il prossimo job pending ordinato per priorità
pub async fn get_next_pending_job(pool: &DbPool) -> Result<Option<JobRecord>, sqlx::Error> {
    sqlx::query_as::<_, JobRecord>(
        r#"
        SELECT id, api_key_id, conversion_type, input_format, output_format,
               quality, status, progress, progress_message, input_path,
               result_path, error, file_size_bytes, created_at, started_at,
               completed_at, updated_at, priority, webhook_url, source_url,
               expires_at, retry_count, original_filename, drive_file_id
        FROM jobs
        WHERE status = 'pending'
        ORDER BY
            CASE priority
                WHEN 'high' THEN 0
                WHEN 'normal' THEN 1
                WHEN 'low' THEN 2
                ELSE 1
            END,
            created_at ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
}

/// Ottieni job scaduti
pub async fn get_expired_jobs(pool: &DbPool) -> Result<Vec<String>, sqlx::Error> {
    let now = Utc::now().to_rfc3339();

    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT id FROM jobs
        WHERE expires_at IS NOT NULL
        AND expires_at < ?
        AND status = 'completed'
        "#,
    )
    .bind(&now)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Ottieni webhook URL per un job
pub async fn get_job_webhook(pool: &DbPool, id: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT webhook_url FROM jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|r| r.0))
}

/// Conta i retry di un job
pub async fn get_job_retry_count(pool: &DbPool, id: &str) -> Result<i64, sqlx::Error> {
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT retry_count FROM jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|r| r.0).unwrap_or(0))
}

/// Ottieni job per un utente (by api_key_id)
pub async fn get_user_jobs(pool: &DbPool, api_key_id: &str, limit: i64) -> Result<Vec<JobRecord>, sqlx::Error> {
    sqlx::query_as::<_, JobRecord>(
        r#"
        SELECT id, api_key_id, conversion_type, input_format, output_format,
               quality, status, progress, progress_message, input_path,
               result_path, error, file_size_bytes, created_at, started_at,
               completed_at, updated_at, priority, webhook_url, source_url,
               expires_at, retry_count, original_filename, drive_file_id
        FROM jobs
        WHERE api_key_id = ?
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(api_key_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Aggiorna drive_file_id per un job
pub async fn update_job_drive_file_id(pool: &DbPool, id: &str, drive_file_id: &str) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET drive_file_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(drive_file_id)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Rimuove drive_file_id da un job (quando il file viene eliminato da Drive)
pub async fn clear_job_drive_file_id(pool: &DbPool, id: &str) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE jobs SET drive_file_id = NULL, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Ottiene il drive_file_id di un job
pub async fn get_job_drive_file_id(pool: &DbPool, id: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT drive_file_id FROM jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(id,)| id))
}
