use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub max_file_size_mb: u64,
    pub temp_dir: PathBuf,
    pub job_retention_hours: u64,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub frontend_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 4000,
            max_file_size_mb: 50,
            temp_dir: std::env::temp_dir().join("converty"),
            job_retention_hours: 24,
            google_client_id: None,
            google_client_secret: None,
            frontend_url: "http://localhost:3000".to_string(),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(host) = std::env::var("CONVERTY_HOST") {
            config.host = host;
        }

        if let Ok(port) = std::env::var("CONVERTY_PORT") {
            if let Ok(p) = port.parse() {
                config.port = p;
            }
        }

        if let Ok(size) = std::env::var("CONVERTY_MAX_FILE_SIZE_MB") {
            if let Ok(s) = size.parse() {
                config.max_file_size_mb = s;
            }
        }

        if let Ok(dir) = std::env::var("CONVERTY_TEMP_DIR") {
            config.temp_dir = PathBuf::from(dir);
        }

        if let Ok(client_id) = std::env::var("GOOGLE_CLIENT_ID") {
            config.google_client_id = Some(client_id);
        }

        if let Ok(client_secret) = std::env::var("GOOGLE_CLIENT_SECRET") {
            config.google_client_secret = Some(client_secret);
        }

        if let Ok(frontend_url) = std::env::var("FRONTEND_URL") {
            config.frontend_url = frontend_url;
        }

        config
    }

    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size_mb * 1024 * 1024
    }
}

// Formati supportati
pub mod formats {
    // Immagini raster
    pub const IMAGE_INPUT: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "ico",
        "avif", "qoi", "pnm", "tga", "dds", "hdr", "exr"
    ];
    pub const IMAGE_OUTPUT: &[&str] = &[
        "png", "jpg", "jpeg", "webp", "bmp", "gif", "avif", "qoi", "tiff"
    ];

    // SVG (vettoriale → raster)
    pub const SVG_INPUT: &[&str] = &["svg"];
    pub const SVG_OUTPUT: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "avif", "qoi"];

    pub const DOCUMENT_INPUT: &[&str] = &["txt", "md", "markdown", "html", "htm"];
    pub const DOCUMENT_OUTPUT: &[&str] = &["pdf", "txt", "html"];

    pub const AUDIO_INPUT: &[&str] = &["mp3", "wav", "ogg", "flac", "aac", "m4a"];
    pub const AUDIO_OUTPUT: &[&str] = &["mp3", "wav", "ogg", "flac"];

    pub const VIDEO_INPUT: &[&str] = &["mp4", "avi", "mkv", "mov", "webm", "wmv"];
    pub const VIDEO_OUTPUT: &[&str] = &["mp4", "webm", "avi", "gif"];

    // PDF → Immagine (richiede pdftoppm/poppler)
    pub const PDF_INPUT: &[&str] = &["pdf"];
    pub const PDF_OUTPUT: &[&str] = &["png", "jpg", "jpeg", "tiff"];

    pub fn is_supported_image_input(ext: &str) -> bool {
        IMAGE_INPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_svg_input(ext: &str) -> bool {
        SVG_INPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_svg_output(ext: &str) -> bool {
        SVG_OUTPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_image_output(ext: &str) -> bool {
        IMAGE_OUTPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_document_input(ext: &str) -> bool {
        DOCUMENT_INPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_document_output(ext: &str) -> bool {
        DOCUMENT_OUTPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_audio_input(ext: &str) -> bool {
        AUDIO_INPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_audio_output(ext: &str) -> bool {
        AUDIO_OUTPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_video_input(ext: &str) -> bool {
        VIDEO_INPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_video_output(ext: &str) -> bool {
        VIDEO_OUTPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_pdf_input(ext: &str) -> bool {
        PDF_INPUT.contains(&ext.to_lowercase().as_str())
    }

    pub fn is_supported_pdf_output(ext: &str) -> bool {
        PDF_OUTPUT.contains(&ext.to_lowercase().as_str())
    }
}
