use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConvertQuery {
    /// Formato di output (es: png, jpg, webp, pdf)
    pub output_format: String,
    /// Qualità output (1-100, default: 85)
    #[serde(default)]
    pub quality: Option<u8>,
    /// Larghezza in pixel (solo immagini)
    #[serde(default)]
    pub width: Option<u32>,
    /// Altezza in pixel (solo immagini)
    #[serde(default)]
    pub height: Option<u32>,
    /// Mantieni proporzioni durante resize (default: true)
    #[serde(default = "default_true")]
    pub maintain_aspect_ratio: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BatchConvertRequest {
    pub output_format: String,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// Priorità del job nella coda
#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize,
    serde::Serialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ToSchema,
    Default,
)]
#[serde(rename_all = "lowercase")]
pub enum JobPriority {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
}

impl std::fmt::Display for JobPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobPriority::Low => write!(f, "low"),
            JobPriority::Normal => write!(f, "normal"),
            JobPriority::High => write!(f, "high"),
        }
    }
}

impl JobPriority {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "high" => JobPriority::High,
            "low" => JobPriority::Low,
            _ => JobPriority::Normal,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateJobRequest {
    pub output_format: String,
    pub conversion_type: ConversionType,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// URL sorgente per scaricare il file (alternativa a upload)
    #[serde(default)]
    pub source_url: Option<String>,
    /// Priorità del job (low, normal, high)
    #[serde(default)]
    pub priority: JobPriority,
    /// URL webhook da chiamare al completamento
    #[serde(default)]
    pub webhook_url: Option<String>,
    /// Tempo di vita risultato in ore (default: 24)
    #[serde(default)]
    pub expires_in_hours: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ConversionType {
    Image,
    Document,
    Audio,
    Video,
    Pdf,
}

impl std::fmt::Display for ConversionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionType::Image => write!(f, "image"),
            ConversionType::Document => write!(f, "document"),
            ConversionType::Audio => write!(f, "audio"),
            ConversionType::Video => write!(f, "video"),
            ConversionType::Pdf => write!(f, "pdf"),
        }
    }
}

/// Opzioni per la trasformazione delle immagini
#[derive(Debug, Clone, Default)]
pub struct ImageOptions {
    pub quality: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub maintain_aspect_ratio: bool,
}

impl ImageOptions {
    pub fn from_query(query: &ConvertQuery) -> Self {
        Self {
            quality: query.quality,
            width: query.width,
            height: query.height,
            maintain_aspect_ratio: query.maintain_aspect_ratio,
        }
    }
}

/// Query parameters per conversione PDF
#[derive(Debug, Deserialize, ToSchema)]
pub struct PdfConvertQuery {
    /// Formato di output (png, jpg, tiff)
    pub output_format: String,
    /// Numero pagina da convertire (default: 1, ignorato se all_pages=true)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Risoluzione DPI (default: 150)
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    /// Converti tutte le pagine e restituisci ZIP (default: false)
    #[serde(default)]
    pub all_pages: bool,
}

fn default_page() -> u32 {
    1
}

fn default_dpi() -> u32 {
    150
}
