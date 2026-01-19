use axum::{
    extract::{Query, State},
    response::Redirect,
    routing::get,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use utoipa::ToSchema;

use crate::db::oauth_users::{self, GoogleUserInfo, OAuthUser};
use crate::db::stats as db_stats;
use crate::db::DbPool;
use crate::error::{AppError, Result};
use crate::models::AuthInfo;

/// State per le route di autenticazione
#[derive(Clone)]
pub struct AuthRouteState {
    pub db: DbPool,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub frontend_url: String,
    /// Cache per i state OAuth (CSRF protection)
    pub oauth_states: std::sync::Arc<RwLock<HashMap<String, std::time::Instant>>>,
}

pub fn router(
    db: DbPool,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    frontend_url: String,
) -> Router {
    let state = AuthRouteState {
        db,
        google_client_id,
        google_client_secret,
        frontend_url,
        oauth_states: std::sync::Arc::new(RwLock::new(HashMap::new())),
    };
    Router::new()
        .route("/api/v1/auth/google/url", get(get_google_auth_url))
        .route("/api/v1/auth/google/callback", get(google_callback))
        .route("/api/v1/auth/me", get(get_current_user))
        .with_state(state)
}

/// Risposta con URL di autenticazione Google
#[derive(Debug, Serialize, ToSchema)]
pub struct GoogleAuthUrlResponse {
    pub url: String,
}

/// Info utente per la risposta
#[derive(Debug, Serialize, ToSchema)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub picture_url: Option<String>,
}

impl From<OAuthUser> for UserInfo {
    fn from(user: OAuthUser) -> Self {
        Self {
            id: user.id,
            email: user.email,
            name: user.name,
            picture_url: user.picture_url,
        }
    }
}

/// Risposta info utente corrente
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentUserResponse {
    pub user: UserInfo,
    pub api_key_prefix: String,
    pub stats: UserStats,
}

/// Statistiche utente
#[derive(Debug, Serialize, ToSchema)]
pub struct UserStats {
    pub total_conversions: u64,
    pub successful: u64,
    pub failed: u64,
    pub bytes_processed: u64,
}

/// Query params per callback Google
#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Risposta token da Google
#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    id_token: Option<String>,
    expires_in: u64,
    #[allow(dead_code)]
    token_type: String,
    refresh_token: Option<String>,
}

/// Info utente da Google
#[derive(Debug, Deserialize)]
struct GoogleUserInfoResponse {
    sub: String,
    email: String,
    name: Option<String>,
    picture: Option<String>,
}

/// Genera URL per autenticazione Google
#[utoipa::path(
    get,
    path = "/api/v1/auth/google/url",
    responses(
        (status = 200, description = "URL di autenticazione Google", body = GoogleAuthUrlResponse),
        (status = 500, description = "Google OAuth non configurato"),
    ),
    tag = "Auth"
)]
pub async fn get_google_auth_url(
    State(state): State<AuthRouteState>,
) -> Result<Json<GoogleAuthUrlResponse>> {
    let client_id = state
        .google_client_id
        .as_ref()
        .ok_or_else(|| AppError::Internal("Google OAuth non configurato".to_string()))?;

    // Genera state casuale per CSRF protection
    let oauth_state = generate_random_state();

    // Salva state con timestamp
    {
        let mut states = state.oauth_states.write().unwrap();
        // Pulisci stati vecchi (> 10 minuti)
        let now = std::time::Instant::now();
        states.retain(|_, timestamp| now.duration_since(*timestamp).as_secs() < 600);
        states.insert(oauth_state.clone(), now);
    }

    let redirect_uri = std::env::var("GOOGLE_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:4000/api/v1/auth/google/callback".to_string());

    // Include drive.file scope for saving converted files to Drive
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
        client_id={}&\
        redirect_uri={}&\
        response_type=code&\
        scope=openid%20email%20profile%20https://www.googleapis.com/auth/drive.file&\
        state={}&\
        access_type=offline&\
        prompt=consent",
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&oauth_state),
    );

    Ok(Json(GoogleAuthUrlResponse { url }))
}

/// Callback da Google OAuth
#[utoipa::path(
    get,
    path = "/api/v1/auth/google/callback",
    params(
        ("code" = Option<String>, Query, description = "Authorization code da Google"),
        ("state" = Option<String>, Query, description = "State per CSRF protection"),
        ("error" = Option<String>, Query, description = "Errore da Google"),
    ),
    responses(
        (status = 302, description = "Redirect al frontend con token"),
    ),
    tag = "Auth"
)]
pub async fn google_callback(
    State(state): State<AuthRouteState>,
    Query(query): Query<GoogleCallbackQuery>,
) -> std::result::Result<Redirect, Redirect> {
    let frontend_url = &state.frontend_url;

    // Funzione helper per redirect con errore
    let error_redirect = |msg: &str| {
        Redirect::temporary(&format!(
            "{}?auth_error={}",
            frontend_url,
            urlencoding::encode(msg)
        ))
    };

    // Controlla errori da Google
    if let Some(error) = query.error {
        return Err(error_redirect(&error));
    }

    // Verifica code e state
    let code = query
        .code
        .ok_or_else(|| error_redirect("Missing authorization code"))?;
    let oauth_state = query
        .state
        .ok_or_else(|| error_redirect("Missing state parameter"))?;

    // Verifica CSRF state
    {
        let mut states = state.oauth_states.write().unwrap();
        if states.remove(&oauth_state).is_none() {
            return Err(error_redirect("Invalid state - possible CSRF attack"));
        }
    }

    // Ottieni credentials
    let client_id = state
        .google_client_id
        .as_ref()
        .ok_or_else(|| error_redirect("Google OAuth not configured"))?;
    let client_secret = state
        .google_client_secret
        .as_ref()
        .ok_or_else(|| error_redirect("Google OAuth not configured"))?;

    // Scambia code per token
    let redirect_uri = std::env::var("GOOGLE_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:4000/api/v1/auth/google/callback".to_string());
    let token_response = exchange_code_for_token(
        &code,
        client_id,
        client_secret,
        &redirect_uri,
    )
    .await
    .map_err(|e| error_redirect(&format!("Token exchange failed: {}", e)))?;

    // Ottieni info utente da Google
    let user_info = get_google_user_info(&token_response.access_token)
        .await
        .map_err(|e| error_redirect(&format!("Failed to get user info: {}", e)))?;

    // Crea o trova utente nel database
    let google_user_info = GoogleUserInfo {
        google_id: user_info.sub,
        email: user_info.email,
        name: user_info.name,
        picture_url: user_info.picture,
    };

    let result = oauth_users::login_or_register(&state.db, google_user_info)
        .await
        .map_err(|e| error_redirect(&format!("Database error: {}", e)))?;

    // Salva i token OAuth per Google Drive
    let _ = oauth_users::save_tokens(
        &state.db,
        &result.user.id,
        &token_response.access_token,
        token_response.refresh_token.as_deref(),
        token_response.expires_in,
    )
    .await;

    // Costruisci URL di redirect con i dati
    let mut redirect_url = format!(
        "{}?auth_success=true&user_id={}&email={}&api_key_prefix={}",
        frontend_url,
        urlencoding::encode(&result.user.id),
        urlencoding::encode(&result.user.email),
        urlencoding::encode(&result.api_key_prefix),
    );

    if let Some(name) = &result.user.name {
        redirect_url.push_str(&format!("&name={}", urlencoding::encode(name)));
    }
    if let Some(picture) = &result.user.picture_url {
        redirect_url.push_str(&format!("&picture={}", urlencoding::encode(picture)));
    }
    if result.is_new_user {
        redirect_url.push_str("&is_new_user=true");
    }
    // Invia sempre la API key (sia per nuovi che per utenti esistenti)
    if let Some(api_key) = &result.api_key {
        redirect_url.push_str(&format!("&api_key={}", urlencoding::encode(api_key)));
    }

    Ok(Redirect::temporary(&redirect_url))
}

/// Scambia authorization code per access token
async fn exchange_code_for_token(
    code: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> std::result::Result<GoogleTokenResponse, String> {
    let client = reqwest::Client::new();

    let params = [
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Token request failed: {}", error_text));
    }

    response
        .json::<GoogleTokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))
}

/// Ottieni info utente da Google
async fn get_google_user_info(
    access_token: &str,
) -> std::result::Result<GoogleUserInfoResponse, String> {
    let client = reqwest::Client::new();

    let response = client
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("User info request failed: {}", error_text));
    }

    response
        .json::<GoogleUserInfoResponse>()
        .await
        .map_err(|e| format!("Failed to parse user info: {}", e))
}

/// Genera stringa casuale per OAuth state
fn generate_random_state() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", timestamp)
}

/// Ottieni info utente corrente
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses(
        (status = 200, description = "Info utente", body = CurrentUserResponse),
        (status = 401, description = "Non autenticato"),
    ),
    security(("api_key" = [])),
    tag = "Auth"
)]
pub async fn get_current_user(
    State(state): State<AuthRouteState>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<CurrentUserResponse>> {
    // Richiede autenticazione
    if auth.is_guest {
        return Err(AppError::Unauthorized(
            "Autenticazione richiesta".to_string(),
        ));
    }

    let api_key_id = auth
        .api_key_id
        .as_ref()
        .ok_or_else(|| AppError::Unauthorized("API Key non trovata".to_string()))?;

    // Trova l'utente OAuth associato all'API key
    let oauth_user = oauth_users::find_by_api_key_id(&state.db, api_key_id)
        .await
        .map_err(|e| AppError::Internal(format!("Errore database: {}", e)))?
        .ok_or_else(|| AppError::NotFound("Utente non trovato".to_string()))?;

    // Ottieni statistiche
    let api_key_stats = db_stats::get_api_key_stats(&state.db, api_key_id)
        .await
        .map_err(|e| AppError::Internal(format!("Errore statistiche: {}", e)))?;

    let stats = if let Some(aks) = api_key_stats {
        UserStats {
            total_conversions: aks.total_conversions,
            successful: aks.successful_conversions,
            failed: aks.failed_conversions,
            bytes_processed: aks.total_input_bytes,
        }
    } else {
        UserStats {
            total_conversions: 0,
            successful: 0,
            failed: 0,
            bytes_processed: 0,
        }
    };

    // Ottieni prefix API key
    let api_key_prefix = oauth_users::get_api_key_prefix(&state.db, api_key_id)
        .await
        .map_err(|e| AppError::Internal(format!("Errore: {}", e)))?
        .unwrap_or_else(|| "cv_...".to_string());

    Ok(Json(CurrentUserResponse {
        user: oauth_user.into(),
        api_key_prefix,
        stats,
    }))
}
