use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::DbPool;
use crate::models::{
    ApiKeyStats, ConversionSummary, FormatCount, FormatStats, GlobalStats, StatsQuery,
    StatsResponse, TimeWindowStats, TypeStats,
};

/// Record conversione per database
#[derive(Debug, Clone)]
pub struct ConversionRecordDb {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub api_key_id: Option<String>,
    pub is_guest: bool,
    pub conversion_type: String,
    pub input_format: String,
    pub output_format: String,
    pub input_size_bytes: i64,
    pub output_size_bytes: i64,
    pub processing_time_ms: i64,
    pub success: bool,
    pub error: Option<String>,
    pub client_ip: Option<String>,
}

/// Inserisce un record di conversione
pub async fn insert_conversion(pool: &DbPool, record: &ConversionRecordDb) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO conversion_records
        (id, timestamp, api_key_id, is_guest, conversion_type, input_format, output_format,
         input_size_bytes, output_size_bytes, processing_time_ms, success, error, client_ip)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.id)
    .bind(record.timestamp.to_rfc3339())
    .bind(&record.api_key_id)
    .bind(if record.is_guest { 1 } else { 0 })
    .bind(&record.conversion_type)
    .bind(&record.input_format)
    .bind(&record.output_format)
    .bind(record.input_size_bytes)
    .bind(record.output_size_bytes)
    .bind(record.processing_time_ms)
    .bind(if record.success { 1 } else { 0 })
    .bind(&record.error)
    .bind(&record.client_ip)
    .execute(pool)
    .await?;
    Ok(())
}

/// Ottiene statistiche globali
pub async fn get_global_stats(pool: &DbPool) -> Result<GlobalStats, sqlx::Error> {
    // Statistiche totali
    let total: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
            SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as failed,
            COALESCE(SUM(input_size_bytes), 0) as input_bytes,
            COALESCE(SUM(output_size_bytes), 0) as output_bytes
        FROM conversion_records
        "#,
    )
    .fetch_one(pool)
    .await?;

    // Tempo medio elaborazione
    let avg_time: (f64,) = sqlx::query_as(
        "SELECT COALESCE(AVG(CAST(processing_time_ms AS REAL)), 0.0) FROM conversion_records"
    )
    .fetch_one(pool)
    .await?;

    // Statistiche per tipo
    let by_type = get_type_stats(pool).await?;

    // Statistiche per formato
    let by_format = get_format_stats(pool).await?;

    // Ultime 24 ore
    let last_24h = get_time_window_stats(pool, Duration::hours(24)).await?;

    // Ultima ora
    let last_hour = get_time_window_stats(pool, Duration::hours(1)).await?;

    Ok(GlobalStats {
        total_conversions: total.0 as u64,
        successful_conversions: total.1 as u64,
        failed_conversions: total.2 as u64,
        total_input_bytes: total.3 as u64,
        total_output_bytes: total.4 as u64,
        avg_processing_time_ms: avg_time.0,
        by_type,
        by_format,
        last_24h,
        last_hour,
    })
}

async fn get_type_stats(pool: &DbPool) -> Result<TypeStats, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT conversion_type, COUNT(*) as count
        FROM conversion_records
        GROUP BY conversion_type
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut stats = TypeStats::default();
    for (typ, count) in rows {
        match typ.as_str() {
            "image" => stats.image = count as u64,
            "document" => stats.document = count as u64,
            "audio" => stats.audio = count as u64,
            "video" => stats.video = count as u64,
            _ => {}
        }
    }
    Ok(stats)
}

async fn get_format_stats(pool: &DbPool) -> Result<FormatStats, sqlx::Error> {
    let input_rows: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT input_format, COUNT(*) as count
        FROM conversion_records
        GROUP BY input_format
        ORDER BY count DESC
        LIMIT 10
        "#,
    )
    .fetch_all(pool)
    .await?;

    let output_rows: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT output_format, COUNT(*) as count
        FROM conversion_records
        GROUP BY output_format
        ORDER BY count DESC
        LIMIT 10
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(FormatStats {
        input_formats: input_rows
            .into_iter()
            .map(|(format, count)| FormatCount {
                format,
                count: count as u64,
            })
            .collect(),
        output_formats: output_rows
            .into_iter()
            .map(|(format, count)| FormatCount {
                format,
                count: count as u64,
            })
            .collect(),
    })
}

async fn get_time_window_stats(pool: &DbPool, duration: Duration) -> Result<TimeWindowStats, sqlx::Error> {
    let since = (Utc::now() - duration).to_rfc3339();

    let stats: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
            SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as failed,
            COALESCE(SUM(input_size_bytes), 0) as bytes
        FROM conversion_records
        WHERE timestamp >= ?
        "#,
    )
    .bind(&since)
    .fetch_one(pool)
    .await?;

    Ok(TimeWindowStats {
        conversions: stats.0 as u64,
        successful: stats.1 as u64,
        failed: stats.2 as u64,
        bytes_processed: stats.3 as u64,
    })
}

/// Ottiene statistiche per una specifica API Key
pub async fn get_api_key_stats(pool: &DbPool, api_key_id: &str) -> Result<Option<ApiKeyStats>, sqlx::Error> {
    // Verifica che l'API key esista
    let key_info: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, created_at FROM api_keys WHERE id = ?"
    )
    .bind(api_key_id)
    .fetch_optional(pool)
    .await?;

    let (key_id, _name, created_at_str) = match key_info {
        Some(info) => info,
        None => return Ok(None),
    };

    // Statistiche totali per questa API key
    let stats: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
            SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as failed,
            COALESCE(SUM(input_size_bytes), 0) as input_bytes,
            COALESCE(SUM(output_size_bytes), 0) as output_bytes
        FROM conversion_records
        WHERE api_key_id = ?
        "#,
    )
    .bind(&key_id)
    .fetch_one(pool)
    .await?;

    // Prima e ultima conversione
    let first_last: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT MIN(timestamp), MAX(timestamp)
        FROM conversion_records
        WHERE api_key_id = ?
        "#,
    )
    .bind(&key_id)
    .fetch_optional(pool)
    .await?;

    let (first_used, last_used) = match first_last {
        Some((first, last)) => (
            DateTime::parse_from_rfc3339(&first)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            DateTime::parse_from_rfc3339(&last)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        ),
        None => {
            let created = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            (created, created)
        }
    };

    // Conversioni oggi
    let today_start = Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
    let today_count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM conversion_records
        WHERE api_key_id = ? AND timestamp >= ?
        "#,
    )
    .bind(&key_id)
    .bind(today_start.to_string())
    .fetch_one(pool)
    .await?;

    // Conversioni ultima ora
    let hour_ago = (Utc::now() - Duration::hours(1)).to_rfc3339();
    let hour_count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM conversion_records
        WHERE api_key_id = ? AND timestamp >= ?
        "#,
    )
    .bind(&key_id)
    .bind(&hour_ago)
    .fetch_one(pool)
    .await?;

    Ok(Some(ApiKeyStats {
        api_key: key_id,
        total_conversions: stats.0 as u64,
        successful_conversions: stats.1 as u64,
        failed_conversions: stats.2 as u64,
        total_input_bytes: stats.3 as u64,
        total_output_bytes: stats.4 as u64,
        first_used,
        last_used,
        conversions_today: today_count.0 as u64,
        conversions_this_hour: hour_count.0 as u64,
    }))
}

/// Ottiene conversioni recenti con filtri
pub async fn get_recent_conversions(
    pool: &DbPool,
    query: &StatsQuery,
    api_key_id: Option<&str>,
) -> Result<Vec<ConversionSummary>, sqlx::Error> {
    let mut sql = String::from(
        r#"
        SELECT id, timestamp, conversion_type, input_format, output_format,
               input_size_bytes, output_size_bytes, processing_time_ms, success
        FROM conversion_records
        WHERE 1=1
        "#
    );

    if let Some(ref conv_type) = query.conversion_type {
        sql.push_str(&format!(" AND conversion_type = '{}'", conv_type));
    }
    if let Some(ref input_fmt) = query.input_format {
        sql.push_str(&format!(" AND input_format = '{}'", input_fmt));
    }
    if let Some(ref output_fmt) = query.output_format {
        sql.push_str(&format!(" AND output_format = '{}'", output_fmt));
    }
    if query.only_failed {
        sql.push_str(" AND success = 0");
    }
    if let Some(key_id) = api_key_id {
        sql.push_str(&format!(" AND api_key_id = '{}'", key_id));
    }

    sql.push_str(" ORDER BY timestamp DESC");
    sql.push_str(&format!(" LIMIT {}", query.limit));

    let rows: Vec<(String, String, String, String, String, i64, i64, i64, i64)> =
        sqlx::query_as(&sql).fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, timestamp, conversion_type, input_format, output_format, input_size, output_size, time_ms, success)| {
                ConversionSummary {
                    id,
                    timestamp: DateTime::parse_from_rfc3339(&timestamp)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    conversion_type,
                    input_format,
                    output_format,
                    input_size_bytes: input_size as u64,
                    output_size_bytes: output_size as u64,
                    processing_time_ms: time_ms as u64,
                    success: success != 0,
                }
            },
        )
        .collect())
}

/// Configurazione guest
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GuestConfig {
    pub enabled: bool,
    pub rate_limit_per_minute: i64,
    pub daily_limit: i64,
    pub max_file_size_mb: i64,
    pub allowed_types: Vec<String>,
}

/// Ottiene configurazione guest
pub async fn get_guest_config(pool: &DbPool) -> Result<GuestConfig, sqlx::Error> {
    let row: (i64, i64, i64, i64, String) = sqlx::query_as(
        "SELECT enabled, rate_limit_per_minute, daily_limit, max_file_size_mb, allowed_types FROM guest_config WHERE id = 1"
    )
    .fetch_one(pool)
    .await?;

    Ok(GuestConfig {
        enabled: row.0 != 0,
        rate_limit_per_minute: row.1,
        daily_limit: row.2,
        max_file_size_mb: row.3,
        allowed_types: row.4.split(',').map(|s| s.trim().to_string()).collect(),
    })
}

/// Aggiorna configurazione guest
pub async fn update_guest_config(pool: &DbPool, config: &GuestConfig) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE guest_config SET
            enabled = ?,
            rate_limit_per_minute = ?,
            daily_limit = ?,
            max_file_size_mb = ?,
            allowed_types = ?,
            updated_at = ?
        WHERE id = 1
        "#,
    )
    .bind(if config.enabled { 1 } else { 0 })
    .bind(config.rate_limit_per_minute)
    .bind(config.daily_limit)
    .bind(config.max_file_size_mb)
    .bind(config.allowed_types.join(","))
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Ottiene uso giornaliero guest per IP
pub async fn get_guest_daily_usage(pool: &DbPool, ip: &str) -> Result<i64, sqlx::Error> {
    let today = Utc::now().format("%Y-%m-%d").to_string();

    let count: Option<(i64,)> = sqlx::query_as(
        "SELECT conversions FROM guest_daily_usage WHERE ip_address = ? AND date = ?"
    )
    .bind(ip)
    .bind(&today)
    .fetch_optional(pool)
    .await?;

    Ok(count.map(|(c,)| c).unwrap_or(0))
}

/// Incrementa uso giornaliero guest
pub async fn increment_guest_usage(pool: &DbPool, ip: &str) -> Result<(), sqlx::Error> {
    let today = Utc::now().format("%Y-%m-%d").to_string();

    sqlx::query(
        r#"
        INSERT INTO guest_daily_usage (ip_address, date, conversions)
        VALUES (?, ?, 1)
        ON CONFLICT(ip_address, date) DO UPDATE SET conversions = conversions + 1
        "#,
    )
    .bind(ip)
    .bind(&today)
    .execute(pool)
    .await?;
    Ok(())
}

/// Pulisce vecchi record (più di 30 giorni)
pub async fn cleanup_old_records(pool: &DbPool, days: i64) -> Result<u64, sqlx::Error> {
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();

    let result = sqlx::query("DELETE FROM conversion_records WHERE timestamp < ?")
        .bind(&cutoff)
        .execute(pool)
        .await?;

    // Pulisci anche guest_daily_usage
    let date_cutoff = (Utc::now() - Duration::days(days)).format("%Y-%m-%d").to_string();
    sqlx::query("DELETE FROM guest_daily_usage WHERE date < ?")
        .bind(&date_cutoff)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

/// Record conversione per history
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConversionHistoryItem {
    pub id: String,
    pub input_format: String,
    pub output_format: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub file_size: i64,
    pub original_filename: Option<String>,
    pub drive_file_id: Option<String>,
}

/// Filtri per history conversioni
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryFilters {
    /// Filtro data: today, week, month, all
    #[serde(default)]
    pub date_filter: Option<String>,
    /// Filtro formato input
    #[serde(default)]
    pub input_format: Option<String>,
    /// Filtro formato output
    #[serde(default)]
    pub output_format: Option<String>,
    /// Filtro stato: completed, failed, all
    #[serde(default)]
    pub status: Option<String>,
}

/// Ottiene le conversioni di un utente (dalla tabella jobs)
pub async fn get_user_conversions(pool: &DbPool, api_key_id: &str, limit: i64) -> Result<Vec<ConversionHistoryItem>, sqlx::Error> {
    get_user_conversions_filtered(pool, api_key_id, limit, None).await
}

/// Ottiene le conversioni di un utente con filtri
pub async fn get_user_conversions_filtered(
    pool: &DbPool,
    api_key_id: &str,
    limit: i64,
    filters: Option<&HistoryFilters>,
) -> Result<Vec<ConversionHistoryItem>, sqlx::Error> {
    let mut sql = String::from(
        r#"
        SELECT id, input_format, output_format, status, created_at, completed_at, file_size_bytes, original_filename, drive_file_id
        FROM jobs
        WHERE api_key_id = ?
        "#
    );

    // Applica filtri
    if let Some(f) = filters {
        // Filtro data
        if let Some(date_filter) = &f.date_filter {
            let now = Utc::now();
            let cutoff = match date_filter.as_str() {
                "today" => (now - Duration::hours(24)).to_rfc3339(),
                "week" => (now - Duration::days(7)).to_rfc3339(),
                "month" => (now - Duration::days(30)).to_rfc3339(),
                _ => String::new(),
            };
            if !cutoff.is_empty() {
                sql.push_str(&format!(" AND created_at >= '{}'", cutoff));
            }
        }

        // Filtro formato input
        if let Some(input_fmt) = &f.input_format {
            if !input_fmt.is_empty() {
                sql.push_str(&format!(" AND input_format = '{}'", input_fmt));
            }
        }

        // Filtro formato output
        if let Some(output_fmt) = &f.output_format {
            if !output_fmt.is_empty() {
                sql.push_str(&format!(" AND output_format = '{}'", output_fmt));
            }
        }

        // Filtro stato
        if let Some(status) = &f.status {
            if status != "all" && !status.is_empty() {
                sql.push_str(&format!(" AND status = '{}'", status));
            }
        }
    }

    sql.push_str(" ORDER BY created_at DESC LIMIT ?");

    let rows: Vec<(String, String, String, String, String, Option<String>, Option<i64>, Option<String>, Option<String>)> = sqlx::query_as(&sql)
        .bind(api_key_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, input_format, output_format, status, created_at, completed_at, file_size, original_filename, drive_file_id)| {
            ConversionHistoryItem {
                id,
                input_format,
                output_format,
                status,
                created_at,
                completed_at,
                file_size: file_size.unwrap_or(0),
                original_filename,
                drive_file_id,
            }
        })
        .collect())
}
