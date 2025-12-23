use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::json;
use std::net::SocketAddr;

use crate::db::api_keys::{self, ApiKeyRole};
use crate::db::DbPool;
use crate::routes::convert::AuthInfo;

/// Stato per il middleware di autenticazione
#[derive(Clone)]
pub struct AuthState {
    pub db: DbPool,
}

/// Middleware per autenticazione API Key con supporto guest
///
/// L'API Key deve essere passata nell'header `X-API-Key`
/// oppure come Authorization Bearer token
///
/// Se nessuna API Key è fornita, l'utente è trattato come guest
/// con limitazioni configurabili
pub async fn api_key_auth(
    State(state): State<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let client_ip = Some(addr.ip().to_string());

    // Controlla header X-API-Key
    let api_key_header = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok());

    // Controlla query parameter api_key
    let api_key_query = request.uri().query().and_then(|q| {
        q.split('&')
            .find(|p| p.starts_with("api_key="))
            .map(|p| p.trim_start_matches("api_key="))
    });

    // Controlla Authorization Bearer
    let api_key_bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    // Ottieni la chiave fornita
    let provided_key = api_key_header.or(api_key_query).or(api_key_bearer);

    let auth_info = match provided_key {
        Some(key) => {
            // Verifica API key nel database
            match api_keys::find_by_key(&state.db, key).await {
                Ok(Some(api_key)) => {
                    if !api_key.is_active {
                        return Err((
                            StatusCode::UNAUTHORIZED,
                            Json(json!({
                                "error": "API Key disattivata",
                                "status": 401
                            })),
                        ));
                    }

                    // Aggiorna ultimo utilizzo
                    let _ = api_keys::update_last_used(&state.db, &api_key.id).await;

                    AuthInfo {
                        api_key_id: Some(api_key.id),
                        is_guest: false,
                        role: api_key.role,
                        client_ip,
                    }
                }
                Ok(None) => {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "error": "API Key non valida",
                            "status": 401
                        })),
                    ));
                }
                Err(e) => {
                    tracing::error!("Errore verifica API key: {}", e);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "Errore interno autenticazione",
                            "status": 500
                        })),
                    ));
                }
            }
        }
        None => {
            // Modalità guest
            AuthInfo {
                api_key_id: None,
                is_guest: true,
                role: ApiKeyRole::User,
                client_ip,
            }
        }
    };

    // Aggiungi informazioni autenticazione come extension
    request.extensions_mut().insert(auth_info.clone());

    // Per le route admin, inserisci anche il ruolo e l'id separatamente
    request.extensions_mut().insert(auth_info.role.clone());
    request
        .extensions_mut()
        .insert(auth_info.api_key_id.clone());

    Ok(next.run(request).await)
}

/// Middleware per richiedere autenticazione (no guest)
pub async fn require_auth(
    Extension(auth): Extension<AuthInfo>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    if auth.is_guest {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Autenticazione richiesta. Fornire una API Key valida.",
                "status": 401
            })),
        ));
    }

    Ok(next.run(request).await)
}

/// Middleware per richiedere privilegi admin
pub async fn require_admin(
    Extension(auth): Extension<AuthInfo>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    if auth.is_guest {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Autenticazione richiesta",
                "status": 401
            })),
        ));
    }

    if auth.role != ApiKeyRole::Admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Privilegi admin richiesti",
                "status": 403
            })),
        ));
    }

    Ok(next.run(request).await)
}
