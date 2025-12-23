use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::api_keys::{self, ApiKeyCreated, CreateApiKeyRequest};
use super::DbPool;

/// OAuth User nel database
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OAuthUser {
    pub id: String,
    pub google_id: String,
    pub email: String,
    pub name: Option<String>,
    pub picture_url: Option<String>,
    pub api_key_id: String,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub last_login_at: DateTime<Utc>,
}

/// Info utente da Google
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleUserInfo {
    pub google_id: String,
    pub email: String,
    pub name: Option<String>,
    pub picture_url: Option<String>,
}

/// Risultato login/registrazione
#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthLoginResult {
    pub user: OAuthUser,
    /// API key in chiaro - solo per nuovi utenti!
    pub api_key: Option<String>,
    pub api_key_prefix: String,
    pub is_new_user: bool,
}

/// Trova utente OAuth per Google ID
pub async fn find_by_google_id(
    pool: &DbPool,
    google_id: &str,
) -> Result<Option<OAuthUser>, sqlx::Error> {
    let row: Option<(
        String, String, String, Option<String>, Option<String>, String, String, String, String
    )> = sqlx::query_as(
        r#"
        SELECT id, google_id, email, name, picture_url, api_key_id, created_at, updated_at, last_login_at
        FROM oauth_users
        WHERE google_id = ?
        "#,
    )
    .bind(google_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((
            id,
            google_id,
            email,
            name,
            picture_url,
            api_key_id,
            created_at,
            updated_at,
            last_login_at,
        )) => Ok(Some(OAuthUser {
            id,
            google_id,
            email,
            name,
            picture_url,
            api_key_id,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_login_at: DateTime::parse_from_rfc3339(&last_login_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })),
        None => Ok(None),
    }
}

/// Trova utente OAuth per API Key ID
pub async fn find_by_api_key_id(
    pool: &DbPool,
    api_key_id: &str,
) -> Result<Option<OAuthUser>, sqlx::Error> {
    let row: Option<(
        String, String, String, Option<String>, Option<String>, String, String, String, String
    )> = sqlx::query_as(
        r#"
        SELECT id, google_id, email, name, picture_url, api_key_id, created_at, updated_at, last_login_at
        FROM oauth_users
        WHERE api_key_id = ?
        "#,
    )
    .bind(api_key_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((
            id,
            google_id,
            email,
            name,
            picture_url,
            api_key_id,
            created_at,
            updated_at,
            last_login_at,
        )) => Ok(Some(OAuthUser {
            id,
            google_id,
            email,
            name,
            picture_url,
            api_key_id,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_login_at: DateTime::parse_from_rfc3339(&last_login_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })),
        None => Ok(None),
    }
}

/// Crea nuovo utente OAuth con API Key associata
pub async fn create_oauth_user(
    pool: &DbPool,
    user_info: &GoogleUserInfo,
) -> Result<(OAuthUser, ApiKeyCreated), sqlx::Error> {
    // Crea API key per l'utente
    let api_key_request = CreateApiKeyRequest {
        name: format!("Google: {}", user_info.email),
        role: "user".to_string(),
        rate_limit: 100,
        daily_limit: Some(500),
        notes: Some(format!(
            "Auto-generated for Google user: {}",
            user_info.google_id
        )),
    };

    let api_key = api_keys::create_api_key(pool, &api_key_request, None).await?;

    // Crea utente OAuth
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO oauth_users (id, google_id, email, name, picture_url, api_key_id, created_at, updated_at, last_login_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&user_info.google_id)
    .bind(&user_info.email)
    .bind(&user_info.name)
    .bind(&user_info.picture_url)
    .bind(&api_key.id)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .execute(pool)
    .await?;

    let oauth_user = OAuthUser {
        id,
        google_id: user_info.google_id.clone(),
        email: user_info.email.clone(),
        name: user_info.name.clone(),
        picture_url: user_info.picture_url.clone(),
        api_key_id: api_key.id.clone(),
        created_at: now,
        updated_at: now,
        last_login_at: now,
    };

    Ok((oauth_user, api_key))
}

/// Aggiorna timestamp ultimo login
pub async fn update_last_login(pool: &DbPool, id: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query(
        r#"
        UPDATE oauth_users SET last_login_at = ?, updated_at = ? WHERE id = ?
        "#,
    )
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Aggiorna info utente da Google (email, nome, foto potrebbero cambiare)
pub async fn update_user_info(
    pool: &DbPool,
    id: &str,
    user_info: &GoogleUserInfo,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query(
        r#"
        UPDATE oauth_users
        SET email = ?, name = ?, picture_url = ?, updated_at = ?, last_login_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&user_info.email)
    .bind(&user_info.name)
    .bind(&user_info.picture_url)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Ottieni API key prefix per un utente
pub async fn get_api_key_prefix(
    pool: &DbPool,
    api_key_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as("SELECT key_prefix FROM api_keys WHERE id = ?")
        .bind(api_key_id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|(prefix,)| prefix))
}

/// Crea nuova API key per utente esistente (quando la vecchia non ha plaintext)
async fn create_new_api_key_for_user(
    pool: &DbPool,
    user_id: &str,
    user_info: &GoogleUserInfo,
) -> Result<ApiKeyCreated, sqlx::Error> {
    let api_key_request = CreateApiKeyRequest {
        name: format!("Google: {}", user_info.email),
        role: "user".to_string(),
        rate_limit: 100,
        daily_limit: Some(500),
        notes: Some(format!(
            "Auto-generated for Google user: {}",
            user_info.google_id
        )),
    };

    let api_key = api_keys::create_api_key(pool, &api_key_request, None).await?;

    // Aggiorna l'utente con la nuova API key
    let now = Utc::now();
    sqlx::query("UPDATE oauth_users SET api_key_id = ?, updated_at = ? WHERE id = ?")
        .bind(&api_key.id)
        .bind(now.to_rfc3339())
        .bind(user_id)
        .execute(pool)
        .await?;

    Ok(api_key)
}

/// Login o registrazione con Google
pub async fn login_or_register(
    pool: &DbPool,
    user_info: GoogleUserInfo,
) -> Result<OAuthLoginResult, sqlx::Error> {
    // Cerca utente esistente
    if let Some(mut existing_user) = find_by_google_id(pool, &user_info.google_id).await? {
        // Aggiorna info e ultimo login
        update_user_info(pool, &existing_user.id, &user_info).await?;

        // Recupera la chiave in chiaro dal DB
        let existing_key = api_keys::get_plaintext_key(pool, &existing_user.api_key_id).await?;

        if let Some(api_key) = existing_key {
            // Chiave trovata con plaintext, la usiamo
            let prefix = get_api_key_prefix(pool, &existing_user.api_key_id)
                .await?
                .unwrap_or_else(|| "cv_...".to_string());

            return Ok(OAuthLoginResult {
                user: existing_user,
                api_key: Some(api_key),
                api_key_prefix: prefix,
                is_new_user: false,
            });
        }

        // Chiave vecchia senza plaintext: creiamo una nuova API key
        let new_api_key = create_new_api_key_for_user(pool, &existing_user.id, &user_info).await?;
        existing_user.api_key_id = new_api_key.id.clone();

        return Ok(OAuthLoginResult {
            user: existing_user,
            api_key: Some(new_api_key.api_key),
            api_key_prefix: new_api_key.key_prefix,
            is_new_user: false,
        });
    }

    // Nuovo utente: crea account e API key
    let (new_user, api_key) = create_oauth_user(pool, &user_info).await?;

    Ok(OAuthLoginResult {
        user: new_user,
        api_key: Some(api_key.api_key),
        api_key_prefix: api_key.key_prefix,
        is_new_user: true,
    })
}

/// Token OAuth per Google Drive
#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Salva i token OAuth per un utente
pub async fn save_tokens(
    pool: &DbPool,
    user_id: &str,
    access_token: &str,
    refresh_token: Option<&str>,
    expires_in_seconds: u64,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    let expires_at = now + chrono::Duration::seconds(expires_in_seconds as i64);

    sqlx::query(
        r#"
        UPDATE oauth_users SET
            access_token = ?,
            refresh_token = COALESCE(?, refresh_token),
            token_expires_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(access_token)
    .bind(refresh_token)
    .bind(expires_at.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Ottiene i token OAuth per un utente
pub async fn get_tokens(pool: &DbPool, user_id: &str) -> Result<Option<OAuthTokens>, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT access_token, refresh_token, token_expires_at
        FROM oauth_users
        WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((Some(access_token), refresh_token, expires_at_str)) => {
            let expires_at = expires_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            Ok(Some(OAuthTokens {
                access_token,
                refresh_token,
                expires_at,
            }))
        }
        _ => Ok(None),
    }
}

/// Controlla se il token è scaduto
pub fn is_token_expired(tokens: &OAuthTokens) -> bool {
    match tokens.expires_at {
        Some(expires_at) => Utc::now() >= expires_at - chrono::Duration::minutes(5),
        None => true, // Se non c'è scadenza, consideriamo scaduto
    }
}

/// Ottieni l'ID utente dall'api_key_id
pub async fn get_user_id_by_api_key(
    pool: &DbPool,
    api_key_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM oauth_users WHERE api_key_id = ?")
        .bind(api_key_id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|(id,)| id))
}
