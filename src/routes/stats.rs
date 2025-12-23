use axum::{
    extract::{Query, State},
    routing::get,
    Extension, Json, Router,
};
use chrono::Utc;

use crate::db::api_keys::ApiKeyRole;
use crate::db::stats as db_stats;
use crate::db::DbPool;
use crate::error::{AppError, Result};
use crate::models::{StatsQuery, StatsResponse, StatsSummary};
use crate::routes::convert::AuthInfo;

#[derive(Clone)]
pub struct StatsState {
    pub db: DbPool,
}

pub fn router(db: DbPool) -> Router {
    let state = StatsState { db };
    Router::new()
        .route("/api/v1/stats", get(get_stats))
        .route("/api/v1/stats/summary", get(get_summary))
        .with_state(state)
}

/// Ottieni statistiche complete
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    params(
        ("conversion_type" = Option<String>, Query, description = "Filtra per tipo: image, document, audio, video"),
        ("input_format" = Option<String>, Query, description = "Filtra per formato input"),
        ("output_format" = Option<String>, Query, description = "Filtra per formato output"),
        ("limit" = Option<usize>, Query, description = "Numero conversioni recenti (default: 20)"),
        ("only_failed" = Option<bool>, Query, description = "Mostra solo conversioni fallite"),
    ),
    responses(
        (status = 200, description = "Statistiche complete", body = StatsResponse),
        (status = 401, description = "API Key non valida"),
    ),
    security(("api_key" = [])),
    tag = "Statistiche"
)]
pub async fn get_stats(
    State(state): State<StatsState>,
    Extension(auth): Extension<AuthInfo>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsResponse>> {
    // Guest può vedere solo statistiche limitate
    if auth.is_guest {
        return get_guest_stats(&state.db).await;
    }

    // Ottieni statistiche globali
    let global = db_stats::get_global_stats(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Se l'utente è autenticato, ottieni le sue statistiche
    let api_key_stats = if let Some(ref key_id) = auth.api_key_id {
        db_stats::get_api_key_stats(&state.db, key_id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
    } else {
        None
    };

    // Conversioni recenti (filtrate per l'utente se non admin)
    let recent_conversions = if auth.role == ApiKeyRole::Admin {
        db_stats::get_recent_conversions(&state.db, &query, None)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
    } else if let Some(ref key_id) = auth.api_key_id {
        db_stats::get_recent_conversions(&state.db, &query, Some(key_id))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
    } else {
        Vec::new()
    };

    Ok(Json(StatsResponse {
        global,
        api_key_stats,
        recent_conversions,
        server_uptime_seconds: 0, // TODO: implementare uptime
        generated_at: Utc::now(),
    }))
}

async fn get_guest_stats(db: &DbPool) -> Result<Json<StatsResponse>> {
    let global = db_stats::get_global_stats(db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(StatsResponse {
        global,
        api_key_stats: None,
        recent_conversions: Vec::new(),
        server_uptime_seconds: 0,
        generated_at: Utc::now(),
    }))
}

/// Ottieni sommario rapido statistiche
#[utoipa::path(
    get,
    path = "/api/v1/stats/summary",
    responses(
        (status = 200, description = "Sommario statistiche", body = StatsSummary),
    ),
    tag = "Statistiche"
)]
pub async fn get_summary(State(state): State<StatsState>) -> Result<Json<StatsSummary>> {
    let global = db_stats::get_global_stats(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(StatsSummary {
        total_conversions: global.total_conversions,
        successful: global.successful_conversions,
        failed: global.failed_conversions,
        success_rate: if global.total_conversions > 0 {
            (global.successful_conversions as f64 / global.total_conversions as f64) * 100.0
        } else {
            100.0
        },
        bytes_processed: global.total_input_bytes,
        bytes_generated: global.total_output_bytes,
        compression_ratio: if global.total_input_bytes > 0 {
            global.total_output_bytes as f64 / global.total_input_bytes as f64
        } else {
            1.0
        },
        avg_processing_time_ms: global.avg_processing_time_ms,
        conversions_last_hour: global.last_hour.conversions,
        conversions_last_24h: global.last_24h.conversions,
        uptime_seconds: 0, // TODO: implementare uptime
    }))
}
