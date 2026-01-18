//! Content-type utilities for HTTP responses

/// Get the MIME content-type for a file format
///
/// # Arguments
/// * `format` - The file format extension (e.g., "png", "pdf", "mp3")
///
/// # Returns
/// The corresponding MIME type string
pub fn get_content_type(format: &str) -> &'static str {
    match format.to_lowercase().as_str() {
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",

        // Documents
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",

        // Archives
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "rar" => "application/vnd.rar",
        "7z" => "application/x-7z-compressed",

        // Audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",

        // Video
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",

        // Default
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_types() {
        assert_eq!(get_content_type("png"), "image/png");
        assert_eq!(get_content_type("jpg"), "image/jpeg");
        assert_eq!(get_content_type("jpeg"), "image/jpeg");
        assert_eq!(get_content_type("gif"), "image/gif");
        assert_eq!(get_content_type("webp"), "image/webp");
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(get_content_type("PNG"), "image/png");
        assert_eq!(get_content_type("Jpg"), "image/jpeg");
        assert_eq!(get_content_type("PDF"), "application/pdf");
    }

    #[test]
    fn test_audio_types() {
        assert_eq!(get_content_type("mp3"), "audio/mpeg");
        assert_eq!(get_content_type("wav"), "audio/wav");
        assert_eq!(get_content_type("ogg"), "audio/ogg");
        assert_eq!(get_content_type("flac"), "audio/flac");
    }

    #[test]
    fn test_video_types() {
        assert_eq!(get_content_type("mp4"), "video/mp4");
        assert_eq!(get_content_type("webm"), "video/webm");
        assert_eq!(get_content_type("avi"), "video/x-msvideo");
    }

    #[test]
    fn test_unknown_type() {
        assert_eq!(get_content_type("xyz"), "application/octet-stream");
        assert_eq!(get_content_type("unknown"), "application/octet-stream");
    }
}
