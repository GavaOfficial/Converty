//! Guest user limits and restrictions

use crate::db::stats;
use crate::db::DbPool;
use crate::error::{AppError, Result};
use crate::models::AuthInfo;

/// Check guest limits for a conversion type
pub async fn check_guest_limits(db: &DbPool, auth: &AuthInfo, conversion_type: &str) -> Result<()> {
    let config = stats::get_guest_config(db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !config.enabled {
        return Err(AppError::Forbidden(
            "Modalità guest disabilitata. Richiedi una API Key.".to_string(),
        ));
    }

    // Verifica tipo conversione permesso
    if !config.allowed_types.iter().any(|t| t == conversion_type) {
        return Err(AppError::Forbidden(format!(
            "Tipo conversione '{}' non permesso per guest. Tipi permessi: {}",
            conversion_type,
            config.allowed_types.join(", ")
        )));
    }

    // Verifica limite giornaliero
    if let Some(ip) = &auth.client_ip {
        let daily_usage = stats::get_guest_daily_usage(db, ip)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        if daily_usage >= config.daily_limit {
            return Err(AppError::DailyLimitExceeded(format!(
                "Limite giornaliero di {} conversioni raggiunto per guest",
                config.daily_limit
            )));
        }
    }

    Ok(())
}

/// Check guest file size limit
pub async fn check_guest_file_size(db: &DbPool, size_bytes: i64) -> Result<()> {
    let config = stats::get_guest_config(db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let max_bytes = config.max_file_size_mb * 1024 * 1024;
    if size_bytes > max_bytes {
        return Err(AppError::FileTooLarge(config.max_file_size_mb as u64));
    }

    Ok(())
}
