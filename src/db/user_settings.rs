//! Modulo per la gestione delle impostazioni utente

use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::DbPool;

/// Impostazioni utente
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserSettings {
    pub user_id: String,
    pub save_to_drive_enabled: bool,
    pub drive_folder_id: Option<String>,
    pub drive_folder_name: String,
    pub auto_save_original_filename: bool,
    /// Filtro tipi conversione per Drive: "all" o lista es. "image,audio,video,document"
    pub drive_filter_types: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Request per aggiornare impostazioni
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateSettingsRequest {
    pub save_to_drive_enabled: Option<bool>,
    pub drive_folder_id: Option<String>,
    pub drive_folder_name: Option<String>,
    pub auto_save_original_filename: Option<bool>,
    /// Filtro tipi conversione per Drive: "all" o lista es. "image,audio,video"
    pub drive_filter_types: Option<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            user_id: String::new(),
            save_to_drive_enabled: false,
            drive_folder_id: None,
            drive_folder_name: "Converty Exports".to_string(),
            auto_save_original_filename: true,
            drive_filter_types: "all".to_string(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }
}

/// Ottiene le impostazioni di un utente
pub async fn get_settings(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<UserSettings>, sqlx::Error> {
    let row: Option<(
        String,
        i64,
        Option<String>,
        Option<String>,
        i64,
        Option<String>,
        String,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT user_id, save_to_drive_enabled, drive_folder_id, drive_folder_name,
               auto_save_original_filename, drive_filter_types, created_at, updated_at
        FROM user_settings
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((
            user_id,
            drive_enabled,
            folder_id,
            folder_name,
            auto_filename,
            filter_types,
            created_at,
            updated_at,
        )) => Ok(Some(UserSettings {
            user_id,
            save_to_drive_enabled: drive_enabled != 0,
            drive_folder_id: folder_id,
            drive_folder_name: folder_name.unwrap_or_else(|| "Converty Exports".to_string()),
            auto_save_original_filename: auto_filename != 0,
            drive_filter_types: filter_types.unwrap_or_else(|| "all".to_string()),
            created_at,
            updated_at,
        })),
        None => Ok(None),
    }
}

/// Ottiene le impostazioni o restituisce default
pub async fn get_or_create_settings(
    pool: &DbPool,
    user_id: &str,
) -> Result<UserSettings, sqlx::Error> {
    if let Some(settings) = get_settings(pool, user_id).await? {
        return Ok(settings);
    }

    // Crea impostazioni default
    let settings = UserSettings {
        user_id: user_id.to_string(),
        ..Default::default()
    };

    create_settings(pool, &settings).await?;
    Ok(settings)
}

/// Crea nuove impostazioni
pub async fn create_settings(pool: &DbPool, settings: &UserSettings) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO user_settings (
            user_id, save_to_drive_enabled, drive_folder_id, drive_folder_name,
            auto_save_original_filename, drive_filter_types, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&settings.user_id)
    .bind(if settings.save_to_drive_enabled { 1 } else { 0 })
    .bind(&settings.drive_folder_id)
    .bind(&settings.drive_folder_name)
    .bind(if settings.auto_save_original_filename {
        1
    } else {
        0
    })
    .bind(&settings.drive_filter_types)
    .bind(&settings.created_at)
    .bind(&settings.updated_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Aggiorna le impostazioni esistenti
pub async fn update_settings(
    pool: &DbPool,
    user_id: &str,
    update: &UpdateSettingsRequest,
) -> Result<UserSettings, sqlx::Error> {
    // Ottieni impostazioni attuali o crea default
    let mut settings = get_or_create_settings(pool, user_id).await?;

    // Applica aggiornamenti
    if let Some(enabled) = update.save_to_drive_enabled {
        settings.save_to_drive_enabled = enabled;
    }
    if let Some(ref folder_id) = update.drive_folder_id {
        settings.drive_folder_id = Some(folder_id.clone());
    }
    if let Some(ref folder_name) = update.drive_folder_name {
        settings.drive_folder_name = folder_name.clone();
    }
    if let Some(auto_filename) = update.auto_save_original_filename {
        settings.auto_save_original_filename = auto_filename;
    }
    if let Some(ref filter_types) = update.drive_filter_types {
        settings.drive_filter_types = filter_types.clone();
    }

    settings.updated_at = Utc::now().to_rfc3339();

    // Salva
    sqlx::query(
        r#"
        UPDATE user_settings SET
            save_to_drive_enabled = ?,
            drive_folder_id = ?,
            drive_folder_name = ?,
            auto_save_original_filename = ?,
            drive_filter_types = ?,
            updated_at = ?
        WHERE user_id = ?
        "#,
    )
    .bind(if settings.save_to_drive_enabled { 1 } else { 0 })
    .bind(&settings.drive_folder_id)
    .bind(&settings.drive_folder_name)
    .bind(if settings.auto_save_original_filename {
        1
    } else {
        0
    })
    .bind(&settings.drive_filter_types)
    .bind(&settings.updated_at)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(settings)
}

/// Controlla se l'utente ha Drive abilitato
pub async fn is_drive_enabled(pool: &DbPool, user_id: &str) -> Result<bool, sqlx::Error> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT save_to_drive_enabled FROM user_settings WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    Ok(row.map(|(v,)| v != 0).unwrap_or(false))
}

/// Ottiene folder ID per Drive
pub async fn get_drive_folder(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT drive_folder_id, drive_folder_name FROM user_settings WHERE user_id = ? AND save_to_drive_enabled = 1"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((Some(id), name)) => Ok(Some((
            id,
            name.unwrap_or_else(|| "Converty Exports".to_string()),
        ))),
        _ => Ok(None),
    }
}

/// Info Drive per upload (folder name e filtri)
#[derive(Debug, Clone)]
pub struct DriveUploadSettings {
    pub folder_name: String,
    pub folder_id: Option<String>,
    pub filter_types: String,
}

/// Ottiene le impostazioni Drive per upload (se abilitato)
pub async fn get_drive_upload_settings(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<DriveUploadSettings>, sqlx::Error> {
    let row: Option<(i64, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT save_to_drive_enabled, drive_folder_id, drive_folder_name, drive_filter_types FROM user_settings WHERE user_id = ?"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((enabled, folder_id, folder_name, filter_types)) if enabled != 0 => {
            Ok(Some(DriveUploadSettings {
                folder_name: folder_name.unwrap_or_else(|| "Converty Exports".to_string()),
                folder_id,
                filter_types: filter_types.unwrap_or_else(|| "all".to_string()),
            }))
        }
        _ => Ok(None),
    }
}

/// Controlla se un tipo di conversione deve essere salvato su Drive
pub fn should_save_to_drive(filter_types: &str, conversion_type: &str) -> bool {
    if filter_types == "all" || filter_types.is_empty() {
        return true;
    }

    // Splitta i filtri per virgola e controlla se il tipo è presente
    filter_types
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .any(|t| t == conversion_type.to_lowercase())
}
