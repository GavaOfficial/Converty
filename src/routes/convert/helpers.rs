//! Helper functions for conversion routes

use crate::db::stats::{self, ConversionRecordDb};
use crate::db::DbPool;
use crate::models::AuthInfo;

/// Record a conversion in the database for statistics
#[allow(clippy::too_many_arguments)]
pub async fn record_conversion(
    db: &DbPool,
    auth: &AuthInfo,
    conversion_type: &str,
    input_format: &str,
    output_format: &str,
    input_size: i64,
    output_size: i64,
    processing_time_ms: i64,
    success: bool,
    error: Option<String>,
) {
    let record = ConversionRecordDb {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        api_key_id: auth.api_key_id.clone(),
        is_guest: auth.is_guest,
        conversion_type: conversion_type.to_string(),
        input_format: input_format.to_string(),
        output_format: output_format.to_string(),
        input_size_bytes: input_size,
        output_size_bytes: output_size,
        processing_time_ms,
        success,
        error,
        client_ip: auth.client_ip.clone(),
    };

    if let Err(e) = stats::insert_conversion(db, &record).await {
        tracing::error!("Errore salvataggio statistiche: {}", e);
    }
}
