use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::models::{
    ApiKeyStats, ConversionRecord, ConversionSummary, FormatCount, FormatStats, GlobalStats,
    StatsQuery, StatsResponse, TimeWindowStats, TypeStats,
};

pub type StatsService = Arc<RwLock<StatsServiceInner>>;

pub fn create_stats_service() -> StatsService {
    Arc::new(RwLock::new(StatsServiceInner::new()))
}

pub struct StatsServiceInner {
    records: Vec<ConversionRecord>,
    start_time: Instant,
    max_records: usize,
}

impl StatsServiceInner {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            start_time: Instant::now(),
            max_records: 10000, // Mantieni ultimi 10k record
        }
    }

    /// Registra una nuova conversione
    pub fn record_conversion(
        &mut self,
        api_key: Option<String>,
        conversion_type: &str,
        input_format: &str,
        output_format: &str,
        input_size_bytes: u64,
        output_size_bytes: u64,
        processing_time_ms: u64,
        success: bool,
        error: Option<String>,
        client_ip: Option<String>,
    ) -> String {
        let id = Uuid::new_v4().to_string();

        let record = ConversionRecord {
            id: id.clone(),
            timestamp: Utc::now(),
            api_key: api_key.map(|k| mask_api_key(&k)),
            conversion_type: conversion_type.to_string(),
            input_format: input_format.to_lowercase(),
            output_format: output_format.to_lowercase(),
            input_size_bytes,
            output_size_bytes,
            processing_time_ms,
            success,
            error,
            client_ip,
        };

        self.records.push(record);

        // Limita dimensione records
        if self.records.len() > self.max_records {
            self.records.remove(0);
        }

        id
    }

    /// Ottieni statistiche globali
    pub fn get_global_stats(&self) -> GlobalStats {
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);
        let one_day_ago = now - Duration::hours(24);

        let total = self.records.len() as u64;
        let successful = self.records.iter().filter(|r| r.success).count() as u64;
        let failed = total - successful;

        let total_input: u64 = self.records.iter().map(|r| r.input_size_bytes).sum();
        let total_output: u64 = self.records.iter().map(|r| r.output_size_bytes).sum();

        let avg_time = if total > 0 {
            self.records.iter().map(|r| r.processing_time_ms).sum::<u64>() as f64 / total as f64
        } else {
            0.0
        };

        // Stats per tipo
        let by_type = TypeStats {
            image: self.records.iter().filter(|r| r.conversion_type == "image").count() as u64,
            document: self.records.iter().filter(|r| r.conversion_type == "document").count() as u64,
            audio: self.records.iter().filter(|r| r.conversion_type == "audio").count() as u64,
            video: self.records.iter().filter(|r| r.conversion_type == "video").count() as u64,
        };

        // Stats per formato
        let by_format = self.get_format_stats();

        // Stats ultime 24h
        let last_24h_records: Vec<_> = self.records.iter().filter(|r| r.timestamp > one_day_ago).collect();
        let last_24h = TimeWindowStats {
            conversions: last_24h_records.len() as u64,
            successful: last_24h_records.iter().filter(|r| r.success).count() as u64,
            failed: last_24h_records.iter().filter(|r| !r.success).count() as u64,
            bytes_processed: last_24h_records.iter().map(|r| r.input_size_bytes).sum(),
        };

        // Stats ultima ora
        let last_hour_records: Vec<_> = self.records.iter().filter(|r| r.timestamp > one_hour_ago).collect();
        let last_hour = TimeWindowStats {
            conversions: last_hour_records.len() as u64,
            successful: last_hour_records.iter().filter(|r| r.success).count() as u64,
            failed: last_hour_records.iter().filter(|r| !r.success).count() as u64,
            bytes_processed: last_hour_records.iter().map(|r| r.input_size_bytes).sum(),
        };

        GlobalStats {
            total_conversions: total,
            successful_conversions: successful,
            failed_conversions: failed,
            total_input_bytes: total_input,
            total_output_bytes: total_output,
            avg_processing_time_ms: avg_time,
            by_type,
            by_format,
            last_24h,
            last_hour,
        }
    }

    /// Ottieni statistiche per API Key
    pub fn get_api_key_stats(&self, api_key: &str) -> Option<ApiKeyStats> {
        let masked_key = mask_api_key(api_key);
        let key_records: Vec<_> = self
            .records
            .iter()
            .filter(|r| r.api_key.as_ref() == Some(&masked_key))
            .collect();

        if key_records.is_empty() {
            return None;
        }

        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);
        let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let today_start = today_start.and_utc();

        let first_used = key_records.iter().map(|r| r.timestamp).min().unwrap();
        let last_used = key_records.iter().map(|r| r.timestamp).max().unwrap();

        Some(ApiKeyStats {
            api_key: masked_key,
            total_conversions: key_records.len() as u64,
            successful_conversions: key_records.iter().filter(|r| r.success).count() as u64,
            failed_conversions: key_records.iter().filter(|r| !r.success).count() as u64,
            total_input_bytes: key_records.iter().map(|r| r.input_size_bytes).sum(),
            total_output_bytes: key_records.iter().map(|r| r.output_size_bytes).sum(),
            first_used,
            last_used,
            conversions_today: key_records.iter().filter(|r| r.timestamp > today_start).count() as u64,
            conversions_this_hour: key_records.iter().filter(|r| r.timestamp > one_hour_ago).count() as u64,
        })
    }

    /// Ottieni conversioni recenti
    pub fn get_recent_conversions(&self, query: &StatsQuery) -> Vec<ConversionSummary> {
        let mut records: Vec<_> = self.records.iter().collect();

        // Filtra per tipo
        if let Some(ref conv_type) = query.conversion_type {
            records.retain(|r| r.conversion_type == *conv_type);
        }

        // Filtra per formato input
        if let Some(ref format) = query.input_format {
            records.retain(|r| r.input_format == *format);
        }

        // Filtra per formato output
        if let Some(ref format) = query.output_format {
            records.retain(|r| r.output_format == *format);
        }

        // Filtra solo fallite
        if query.only_failed {
            records.retain(|r| !r.success);
        }

        // Ordina per timestamp decrescente e prendi limit
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        records.truncate(query.limit);

        records.into_iter().map(ConversionSummary::from).collect()
    }

    /// Ottieni risposta completa statistiche
    pub fn get_stats_response(&self, api_key: Option<&str>, query: &StatsQuery) -> StatsResponse {
        StatsResponse {
            global: self.get_global_stats(),
            api_key_stats: api_key.and_then(|k| self.get_api_key_stats(k)),
            recent_conversions: self.get_recent_conversions(query),
            server_uptime_seconds: self.start_time.elapsed().as_secs(),
            generated_at: Utc::now(),
        }
    }

    fn get_format_stats(&self) -> FormatStats {
        let mut input_counts: HashMap<String, u64> = HashMap::new();
        let mut output_counts: HashMap<String, u64> = HashMap::new();

        for record in &self.records {
            *input_counts.entry(record.input_format.clone()).or_insert(0) += 1;
            *output_counts.entry(record.output_format.clone()).or_insert(0) += 1;
        }

        let mut input_formats: Vec<_> = input_counts
            .into_iter()
            .map(|(format, count)| FormatCount { format, count })
            .collect();
        input_formats.sort_by(|a, b| b.count.cmp(&a.count));

        let mut output_formats: Vec<_> = output_counts
            .into_iter()
            .map(|(format, count)| FormatCount { format, count })
            .collect();
        output_formats.sort_by(|a, b| b.count.cmp(&a.count));

        FormatStats {
            input_formats,
            output_formats,
        }
    }

    /// Uptime del server
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

/// Maschera API key per privacy (mostra solo primi/ultimi 4 caratteri)
fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

/// Helper per creare un record di conversione
pub struct ConversionTracker {
    start_time: Instant,
    api_key: Option<String>,
    conversion_type: String,
    input_format: String,
    output_format: String,
    input_size: u64,
    client_ip: Option<String>,
}

impl ConversionTracker {
    pub fn new(
        api_key: Option<String>,
        conversion_type: &str,
        input_format: &str,
        output_format: &str,
        input_size: u64,
        client_ip: Option<String>,
    ) -> Self {
        Self {
            start_time: Instant::now(),
            api_key,
            conversion_type: conversion_type.to_string(),
            input_format: input_format.to_string(),
            output_format: output_format.to_string(),
            input_size,
            client_ip,
        }
    }

    pub async fn finish_success(self, stats: &StatsService, output_size: u64) -> String {
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        let mut stats = stats.write().await;
        stats.record_conversion(
            self.api_key,
            &self.conversion_type,
            &self.input_format,
            &self.output_format,
            self.input_size,
            output_size,
            elapsed,
            true,
            None,
            self.client_ip,
        )
    }

    pub async fn finish_error(self, stats: &StatsService, error: &str) -> String {
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        let mut stats = stats.write().await;
        stats.record_conversion(
            self.api_key,
            &self.conversion_type,
            &self.input_format,
            &self.output_format,
            self.input_size,
            0,
            elapsed,
            false,
            Some(error.to_string()),
            self.client_ip,
        )
    }
}
