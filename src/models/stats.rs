use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Record di una singola conversione
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionRecord {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub api_key: Option<String>,
    pub conversion_type: String,
    pub input_format: String,
    pub output_format: String,
    pub input_size_bytes: u64,
    pub output_size_bytes: u64,
    pub processing_time_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub client_ip: Option<String>,
}

/// Statistiche globali
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct GlobalStats {
    /// Totale conversioni effettuate
    pub total_conversions: u64,
    /// Conversioni riuscite
    pub successful_conversions: u64,
    /// Conversioni fallite
    pub failed_conversions: u64,
    /// Byte totali processati in input
    pub total_input_bytes: u64,
    /// Byte totali generati in output
    pub total_output_bytes: u64,
    /// Tempo medio di elaborazione (ms)
    pub avg_processing_time_ms: f64,
    /// Conversioni per tipo
    pub by_type: TypeStats,
    /// Conversioni per formato
    pub by_format: FormatStats,
    /// Statistiche ultime 24 ore
    pub last_24h: TimeWindowStats,
    /// Statistiche ultima ora
    pub last_hour: TimeWindowStats,
}

/// Statistiche per tipo di conversione
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct TypeStats {
    pub image: u64,
    pub document: u64,
    pub audio: u64,
    pub video: u64,
}

/// Statistiche per formato
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct FormatStats {
    pub input_formats: Vec<FormatCount>,
    pub output_formats: Vec<FormatCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FormatCount {
    pub format: String,
    pub count: u64,
}

/// Statistiche per finestra temporale
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct TimeWindowStats {
    pub conversions: u64,
    pub successful: u64,
    pub failed: u64,
    pub bytes_processed: u64,
}

/// Statistiche per API Key
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyStats {
    pub api_key: String,
    pub total_conversions: u64,
    pub successful_conversions: u64,
    pub failed_conversions: u64,
    pub total_input_bytes: u64,
    pub total_output_bytes: u64,
    #[schema(value_type = String, format = "date-time")]
    pub first_used: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub last_used: DateTime<Utc>,
    pub conversions_today: u64,
    pub conversions_this_hour: u64,
}

/// Risposta dettagliata statistiche
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatsResponse {
    pub global: GlobalStats,
    pub api_key_stats: Option<ApiKeyStats>,
    pub recent_conversions: Vec<ConversionSummary>,
    pub server_uptime_seconds: u64,
    #[schema(value_type = String, format = "date-time")]
    pub generated_at: DateTime<Utc>,
}

/// Sommario conversione per lista recenti
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConversionSummary {
    pub id: String,
    #[schema(value_type = String, format = "date-time")]
    pub timestamp: DateTime<Utc>,
    pub conversion_type: String,
    pub input_format: String,
    pub output_format: String,
    pub input_size_bytes: u64,
    pub output_size_bytes: u64,
    pub processing_time_ms: u64,
    pub success: bool,
}

impl From<&ConversionRecord> for ConversionSummary {
    fn from(record: &ConversionRecord) -> Self {
        Self {
            id: record.id.clone(),
            timestamp: record.timestamp,
            conversion_type: record.conversion_type.clone(),
            input_format: record.input_format.clone(),
            output_format: record.output_format.clone(),
            input_size_bytes: record.input_size_bytes,
            output_size_bytes: record.output_size_bytes,
            processing_time_ms: record.processing_time_ms,
            success: record.success,
        }
    }
}

/// Query per filtrare statistiche
#[derive(Debug, Deserialize, ToSchema)]
pub struct StatsQuery {
    /// Filtra per tipo conversione
    #[serde(default)]
    pub conversion_type: Option<String>,
    /// Filtra per formato input
    #[serde(default)]
    pub input_format: Option<String>,
    /// Filtra per formato output
    #[serde(default)]
    pub output_format: Option<String>,
    /// Numero di conversioni recenti da mostrare
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Solo conversioni fallite
    #[serde(default)]
    pub only_failed: bool,
}

fn default_limit() -> usize {
    20
}

/// Sommario rapido statistiche
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatsSummary {
    /// Totale conversioni
    pub total_conversions: u64,
    /// Conversioni riuscite
    pub successful: u64,
    /// Conversioni fallite
    pub failed: u64,
    /// Percentuale successo (0-100)
    pub success_rate: f64,
    /// Byte totali processati
    pub bytes_processed: u64,
    /// Byte totali generati
    pub bytes_generated: u64,
    /// Rapporto compressione (output/input)
    pub compression_ratio: f64,
    /// Tempo medio elaborazione (ms)
    pub avg_processing_time_ms: f64,
    /// Conversioni ultima ora
    pub conversions_last_hour: u64,
    /// Conversioni ultime 24 ore
    pub conversions_last_24h: u64,
    /// Tempo di uptime server (secondi)
    pub uptime_seconds: u64,
}
