//! Routes per gestione impostazioni utente

use axum::{
    extract::State,
    routing::{get, put},
    Extension, Json, Router,
};

use crate::db::oauth_users;
use crate::db::user_settings::{self, UpdateSettingsRequest, UserSettings};
use crate::db::DbPool;
use crate::error::{AppError, Result};
use crate::models::AuthInfo;

/// Stato condiviso per le routes delle impostazioni
#[derive(Clone)]
pub struct SettingsState {
    pub db: DbPool,
}

pub fn router(db: DbPool) -> Router {
    let state = SettingsState { db };

    Router::new()
        .route("/api/v1/settings", get(get_settings))
        .route("/api/v1/settings", put(update_settings))
        .with_state(state)
}

/// Ottieni le impostazioni dell'utente corrente
#[utoipa::path(
    get,
    path = "/api/v1/settings",
    tag = "Settings",
    responses(
        (status = 200, description = "Impostazioni utente", body = UserSettings),
        (status = 401, description = "Non autenticato"),
    )
)]
pub async fn get_settings(
    State(state): State<SettingsState>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<UserSettings>> {
    // Richiede autenticazione
    let api_key_id = auth
        .api_key_id
        .ok_or_else(|| AppError::Unauthorized("Autenticazione richiesta".to_string()))?;

    // Trova l'utente OAuth associato all'API key
    let user = oauth_users::find_by_api_key_id(&state.db, &api_key_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorized("Utente non trovato".to_string()))?;

    // Ottieni o crea impostazioni
    let settings = user_settings::get_or_create_settings(&state.db, &user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(settings))
}

/// Aggiorna le impostazioni dell'utente corrente
#[utoipa::path(
    put,
    path = "/api/v1/settings",
    tag = "Settings",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "Impostazioni aggiornate", body = UserSettings),
        (status = 401, description = "Non autenticato"),
    )
)]
pub async fn update_settings(
    State(state): State<SettingsState>,
    Extension(auth): Extension<AuthInfo>,
    Json(update): Json<UpdateSettingsRequest>,
) -> Result<Json<UserSettings>> {
    // Richiede autenticazione
    let api_key_id = auth
        .api_key_id
        .ok_or_else(|| AppError::Unauthorized("Autenticazione richiesta".to_string()))?;

    // Trova l'utente OAuth associato all'API key
    let user = oauth_users::find_by_api_key_id(&state.db, &api_key_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorized("Utente non trovato".to_string()))?;

    // Aggiorna impostazioni
    let settings = user_settings::update_settings(&state.db, &user.id, &update)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(settings))
}
