use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::api_keys::{
    self, ApiKey, ApiKeyCreated, ApiKeyRole, CreateApiKeyRequest, UpdateApiKeyRequest,
};
use crate::db::stats::{self, GuestConfig};
use crate::db::DbPool;
use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct AdminState {
    pub db: DbPool,
}

pub fn router(db: DbPool) -> Router {
    let state = AdminState { db };
    Router::new()
        // API Keys management
        .route("/api/v1/admin/keys", get(list_api_keys))
        .route("/api/v1/admin/keys", post(create_api_key))
        .route("/api/v1/admin/keys/{id}", get(get_api_key))
        .route("/api/v1/admin/keys/{id}", put(update_api_key))
        .route("/api/v1/admin/keys/{id}", delete(delete_api_key))
        // Guest configuration
        .route("/api/v1/admin/guest", get(get_guest_config))
        .route("/api/v1/admin/guest", put(update_guest_config))
        // Maintenance
        .route("/api/v1/admin/cleanup", post(cleanup_old_data))
        .with_state(state)
}

/// Lista tutte le API Keys
#[utoipa::path(
    get,
    path = "/api/v1/admin/keys",
    responses(
        (status = 200, description = "Lista API Keys", body = Vec<ApiKey>),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn list_api_keys(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
) -> Result<Json<Vec<ApiKey>>> {
    require_admin(&role)?;

    let keys = api_keys::list_all(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(keys))
}

/// Crea una nuova API Key
#[utoipa::path(
    post,
    path = "/api/v1/admin/keys",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "API Key creata", body = ApiKeyCreated),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn create_api_key(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
    Extension(creator_id): Extension<Option<String>>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyCreated>)> {
    require_admin(&role)?;

    let key = api_keys::create_api_key(&state.db, &request, creator_id.as_deref())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok((StatusCode::CREATED, Json(key)))
}

/// Ottieni dettagli API Key
#[utoipa::path(
    get,
    path = "/api/v1/admin/keys/{id}",
    params(
        ("id" = String, Path, description = "ID API Key")
    ),
    responses(
        (status = 200, description = "Dettagli API Key", body = ApiKey),
        (status = 404, description = "Non trovata"),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn get_api_key(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
    Path(id): Path<String>,
) -> Result<Json<ApiKeyWithStats>> {
    require_admin(&role)?;

    // Trova la chiave
    let keys = api_keys::list_all(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let key = keys
        .into_iter()
        .find(|k| k.id == id)
        .ok_or_else(|| AppError::NotFound("API Key non trovata".to_string()))?;

    // Ottieni statistiche
    let stats = stats::get_api_key_stats(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiKeyWithStats { key, stats }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyWithStats {
    #[serde(flatten)]
    pub key: ApiKey,
    pub stats: Option<crate::models::ApiKeyStats>,
}

/// Aggiorna API Key
#[utoipa::path(
    put,
    path = "/api/v1/admin/keys/{id}",
    params(
        ("id" = String, Path, description = "ID API Key")
    ),
    request_body = UpdateApiKeyRequest,
    responses(
        (status = 200, description = "API Key aggiornata"),
        (status = 404, description = "Non trovata"),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn update_api_key(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
    Path(id): Path<String>,
    Json(request): Json<UpdateApiKeyRequest>,
) -> Result<Json<MessageResponse>> {
    require_admin(&role)?;

    let updated = api_keys::update_api_key(&state.db, &id, &request)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if updated {
        Ok(Json(MessageResponse {
            message: "API Key aggiornata".to_string(),
        }))
    } else {
        Err(AppError::NotFound("API Key non trovata".to_string()))
    }
}

/// Elimina API Key
#[utoipa::path(
    delete,
    path = "/api/v1/admin/keys/{id}",
    params(
        ("id" = String, Path, description = "ID API Key")
    ),
    responses(
        (status = 200, description = "API Key eliminata"),
        (status = 404, description = "Non trovata"),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn delete_api_key(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
    Path(id): Path<String>,
) -> Result<Json<MessageResponse>> {
    require_admin(&role)?;

    let deleted = api_keys::delete_api_key(&state.db, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if deleted {
        Ok(Json(MessageResponse {
            message: "API Key eliminata".to_string(),
        }))
    } else {
        Err(AppError::NotFound("API Key non trovata".to_string()))
    }
}

/// Ottieni configurazione guest
#[utoipa::path(
    get,
    path = "/api/v1/admin/guest",
    responses(
        (status = 200, description = "Configurazione guest", body = GuestConfig),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn get_guest_config(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
) -> Result<Json<GuestConfig>> {
    require_admin(&role)?;

    let config = stats::get_guest_config(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(config))
}

/// Aggiorna configurazione guest
#[utoipa::path(
    put,
    path = "/api/v1/admin/guest",
    request_body = GuestConfig,
    responses(
        (status = 200, description = "Configurazione aggiornata"),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn update_guest_config(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
    Json(config): Json<GuestConfig>,
) -> Result<Json<MessageResponse>> {
    require_admin(&role)?;

    stats::update_guest_config(&state.db, &config)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(MessageResponse {
        message: "Configurazione guest aggiornata".to_string(),
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CleanupRequest {
    /// Giorni di dati da mantenere (default: 30)
    #[serde(default = "default_cleanup_days")]
    pub days: i64,
}

fn default_cleanup_days() -> i64 {
    30
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CleanupResponse {
    pub records_deleted: u64,
    pub message: String,
}

/// Pulisci vecchi record
#[utoipa::path(
    post,
    path = "/api/v1/admin/cleanup",
    request_body = CleanupRequest,
    responses(
        (status = 200, description = "Pulizia completata", body = CleanupResponse),
        (status = 401, description = "Non autorizzato"),
        (status = 403, description = "Solo admin"),
    ),
    security(("api_key" = [])),
    tag = "Admin"
)]
pub async fn cleanup_old_data(
    State(state): State<AdminState>,
    Extension(role): Extension<ApiKeyRole>,
    Json(request): Json<CleanupRequest>,
) -> Result<Json<CleanupResponse>> {
    require_admin(&role)?;

    let deleted = stats::cleanup_old_records(&state.db, request.days)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(CleanupResponse {
        records_deleted: deleted,
        message: format!(
            "Eliminati {} record più vecchi di {} giorni",
            deleted, request.days
        ),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

fn require_admin(role: &ApiKeyRole) -> Result<()> {
    if *role != ApiKeyRole::Admin {
        return Err(AppError::Forbidden(
            "Questa operazione richiede privilegi admin".to_string(),
        ));
    }
    Ok(())
}
