//! Servizio per integrazione Google Drive

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::db::oauth_users::{self, OAuthTokens};
use crate::db::DbPool;

const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";
const DRIVE_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3";

/// Errori del servizio Google Drive
#[derive(Debug)]
pub enum DriveError {
    NoTokens,
    TokenExpired,
    RefreshFailed(String),
    ApiFailed(String),
    UploadFailed(String),
}

impl std::fmt::Display for DriveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriveError::NoTokens => write!(f, "No OAuth tokens available"),
            DriveError::TokenExpired => write!(f, "OAuth token expired and refresh failed"),
            DriveError::RefreshFailed(msg) => write!(f, "Token refresh failed: {}", msg),
            DriveError::ApiFailed(msg) => write!(f, "Drive API failed: {}", msg),
            DriveError::UploadFailed(msg) => write!(f, "Upload failed: {}", msg),
        }
    }
}

impl std::error::Error for DriveError {}

/// Risposta refresh token
#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    expires_in: u64,
}

/// Metadati file Drive
#[derive(Debug, Serialize)]
struct FileMetadata {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parents: Option<Vec<String>>,
}

/// Risposta creazione file
#[derive(Debug, Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(rename = "webViewLink")]
    pub web_view_link: Option<String>,
}

/// Risposta ricerca folder
#[derive(Debug, Deserialize)]
struct FileListResponse {
    files: Vec<DriveFile>,
}

/// Servizio Google Drive
pub struct GoogleDriveService {
    client: reqwest::Client,
}

impl GoogleDriveService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Ottiene un token valido, refreshando se necessario
    pub async fn get_valid_token(
        &self,
        pool: &DbPool,
        user_id: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<String, DriveError> {
        let tokens = oauth_users::get_tokens(pool, user_id)
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?
            .ok_or(DriveError::NoTokens)?;

        // Se il token non è scaduto, usalo
        if !oauth_users::is_token_expired(&tokens) {
            return Ok(tokens.access_token);
        }

        // Altrimenti, refresh
        let refresh_token = tokens.refresh_token.ok_or(DriveError::TokenExpired)?;
        self.refresh_token(pool, user_id, &refresh_token, client_id, client_secret)
            .await
    }

    /// Refresh del token
    async fn refresh_token(
        &self,
        pool: &DbPool,
        user_id: &str,
        refresh_token: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<String, DriveError> {
        let params = [
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| DriveError::RefreshFailed(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(DriveError::RefreshFailed(error));
        }

        let token_response: RefreshTokenResponse = response
            .json()
            .await
            .map_err(|e| DriveError::RefreshFailed(e.to_string()))?;

        // Salva il nuovo token
        oauth_users::save_tokens(
            pool,
            user_id,
            &token_response.access_token,
            Some(refresh_token),
            token_response.expires_in,
        )
        .await
        .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        Ok(token_response.access_token)
    }

    /// Trova o crea una cartella su Drive
    pub async fn ensure_folder(
        &self,
        access_token: &str,
        folder_name: &str,
    ) -> Result<String, DriveError> {
        // Prima cerca se la cartella esiste già
        let query = format!(
            "name = '{}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
            folder_name
        );
        let url = format!(
            "{}/files?q={}&fields=files(id,name)",
            DRIVE_API_BASE,
            urlencoding::encode(&query)
        );

        let response = self
            .client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(DriveError::ApiFailed(format!("Search failed: {}", error)));
        }

        let file_list: FileListResponse = response
            .json()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        // Se esiste, ritorna l'ID
        if let Some(folder) = file_list.files.first() {
            return Ok(folder.id.clone());
        }

        // Altrimenti crea la cartella
        let metadata = serde_json::json!({
            "name": folder_name,
            "mimeType": "application/vnd.google-apps.folder"
        });

        let response = self
            .client
            .post(&format!("{}/files", DRIVE_API_BASE))
            .bearer_auth(access_token)
            .json(&metadata)
            .send()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(DriveError::ApiFailed(format!(
                "Create folder failed: {}",
                error
            )));
        }

        let folder: DriveFile = response
            .json()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        Ok(folder.id)
    }

    /// Carica un file su Drive
    pub async fn upload_file(
        &self,
        access_token: &str,
        folder_id: &str,
        filename: &str,
        data: Vec<u8>,
        mime_type: &str,
    ) -> Result<DriveFile, DriveError> {
        let metadata = FileMetadata {
            name: filename.to_string(),
            parents: Some(vec![folder_id.to_string()]),
        };

        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|e| DriveError::UploadFailed(e.to_string()))?;

        // Multipart upload
        let boundary = "converty_upload_boundary";
        let mut body = Vec::new();

        // Parte 1: Metadati JSON
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        body.extend_from_slice(metadata_json.as_bytes());
        body.extend_from_slice(b"\r\n");

        // Parte 2: File binario
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", mime_type).as_bytes());
        body.extend_from_slice(&data);
        body.extend_from_slice(b"\r\n");

        // Chiusura
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let url = format!(
            "{}/files?uploadType=multipart&fields=id,name,webViewLink",
            DRIVE_UPLOAD_BASE
        );

        let response = self
            .client
            .post(&url)
            .bearer_auth(access_token)
            .header(
                "Content-Type",
                format!("multipart/related; boundary={}", boundary),
            )
            .body(body)
            .send()
            .await
            .map_err(|e| DriveError::UploadFailed(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(DriveError::UploadFailed(format!(
                "Upload failed: {}",
                error
            )));
        }

        let file: DriveFile = response
            .json()
            .await
            .map_err(|e| DriveError::UploadFailed(e.to_string()))?;

        Ok(file)
    }

    /// Carica un file da path su Drive
    pub async fn upload_file_from_path(
        &self,
        access_token: &str,
        folder_id: &str,
        file_path: &Path,
        filename: Option<&str>,
    ) -> Result<DriveFile, DriveError> {
        let data = std::fs::read(file_path)
            .map_err(|e| DriveError::UploadFailed(format!("Failed to read file: {}", e)))?;

        let name = filename.unwrap_or_else(|| {
            file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("converted_file")
        });

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let mime_type = get_mime_type(ext);

        self.upload_file(access_token, folder_id, name, data, mime_type)
            .await
    }

    /// Ottiene la thumbnail di un file da Drive
    pub async fn get_thumbnail(
        &self,
        access_token: &str,
        file_id: &str,
        size: u32,
    ) -> Result<Vec<u8>, DriveError> {
        // Prima ottieni il thumbnailLink dal file metadata
        let url = format!("{}/files/{}?fields=thumbnailLink", DRIVE_API_BASE, file_id);

        let response = self
            .client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(DriveError::ApiFailed(format!(
                "Get file metadata failed: {}",
                error
            )));
        }

        #[derive(Deserialize)]
        struct ThumbnailResponse {
            #[serde(rename = "thumbnailLink")]
            thumbnail_link: Option<String>,
        }

        let metadata: ThumbnailResponse = response
            .json()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        // Se non c'è thumbnailLink, prova con l'export diretto per immagini
        let thumbnail_url = if let Some(link) = metadata.thumbnail_link {
            // Il thumbnailLink ha un parametro =s220 per la dimensione, lo modifichiamo
            link.replace("=s220", &format!("=s{}", size))
        } else {
            // Fallback: prova a scaricare il file se è un'immagine piccola
            return Err(DriveError::ApiFailed("No thumbnail available".to_string()));
        };

        // Scarica la thumbnail
        let thumb_response = self
            .client
            .get(&thumbnail_url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        if !thumb_response.status().is_success() {
            return Err(DriveError::ApiFailed(
                "Failed to download thumbnail".to_string(),
            ));
        }

        let bytes = thumb_response
            .bytes()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        Ok(bytes.to_vec())
    }

    /// Elimina un file da Drive
    pub async fn delete_file(&self, access_token: &str, file_id: &str) -> Result<(), DriveError> {
        let url = format!("{}/files/{}", DRIVE_API_BASE, file_id);

        let response = self
            .client
            .delete(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| DriveError::ApiFailed(e.to_string()))?;

        // 204 No Content = success, 404 = file already deleted (also ok)
        if response.status().is_success() || response.status().as_u16() == 404 {
            Ok(())
        } else {
            let error = response.text().await.unwrap_or_default();
            Err(DriveError::ApiFailed(format!("Delete failed: {}", error)))
        }
    }
}

impl Default for GoogleDriveService {
    fn default() -> Self {
        Self::new()
    }
}

/// Ottiene il MIME type da estensione
fn get_mime_type(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" => "text/plain",
        "html" => "text/html",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "tiff" | "tif" => "image/tiff",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
