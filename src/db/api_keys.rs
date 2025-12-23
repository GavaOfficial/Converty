use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use super::DbPool;

/// Ruoli disponibili per API Key
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyRole {
    Admin,
    User,
}

impl std::fmt::Display for ApiKeyRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiKeyRole::Admin => write!(f, "admin"),
            ApiKeyRole::User => write!(f, "user"),
        }
    }
}

impl From<&str> for ApiKeyRole {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "admin" => ApiKeyRole::Admin,
            _ => ApiKeyRole::User,
        }
    }
}

/// API Key nel database
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub key_prefix: String,
    pub role: ApiKeyRole,
    pub is_active: bool,
    pub rate_limit: i64,
    pub daily_limit: Option<i64>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = "date-time")]
    pub updated_at: DateTime<Utc>,
    #[schema(value_type = Option<String>, format = "date-time")]
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
    pub notes: Option<String>,
}

/// Risposta creazione API Key (include la chiave in chiaro una sola volta)
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyCreated {
    pub id: String,
    pub name: String,
    /// La chiave API in chiaro - mostrata solo una volta!
    pub api_key: String,
    pub key_prefix: String,
    pub role: ApiKeyRole,
    pub rate_limit: i64,
    pub daily_limit: Option<i64>,
    #[schema(value_type = String, format = "date-time")]
    pub created_at: DateTime<Utc>,
}

/// Request per creare una nuova API Key
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    /// Nome descrittivo per l'API Key
    pub name: String,
    /// Ruolo: "admin" o "user"
    #[serde(default = "default_role")]
    pub role: String,
    /// Limite richieste al minuto (default: 100)
    #[serde(default = "default_rate_limit")]
    pub rate_limit: i64,
    /// Limite giornaliero (opzionale)
    pub daily_limit: Option<i64>,
    /// Note aggiuntive
    pub notes: Option<String>,
}

fn default_role() -> String {
    "user".to_string()
}

fn default_rate_limit() -> i64 {
    100
}

/// Request per aggiornare un'API Key
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateApiKeyRequest {
    pub name: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit: Option<i64>,
    pub daily_limit: Option<i64>,
    pub notes: Option<String>,
}

/// Genera una nuova API Key sicura
pub fn generate_api_key() -> (String, String, String) {
    let mut rng = rand::thread_rng();
    let key_bytes: [u8; 32] = rng.gen();
    let key = format!(
        "cv_{}",
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, key_bytes)
    );
    let prefix = key[..12].to_string();
    let hash = hash_api_key(&key);
    (key, prefix, hash)
}

/// Hash di una API Key
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Crea una nuova API Key
pub async fn create_api_key(
    pool: &DbPool,
    request: &CreateApiKeyRequest,
    created_by: Option<&str>,
) -> Result<ApiKeyCreated, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let (key, prefix, hash) = generate_api_key();
    let now = Utc::now();
    let role = ApiKeyRole::from(request.role.as_str());

    // Solo per utenti normali: salva anche la chiave in chiaro per poterla recuperare
    // Admin keys restano solo hashate per sicurezza
    let key_plaintext: Option<&str> = if role == ApiKeyRole::User {
        Some(&key)
    } else {
        None
    };

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, name, key_hash, key_prefix, role, is_active, rate_limit, daily_limit, created_at, updated_at, created_by, notes, key_plaintext)
        VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&request.name)
    .bind(&hash)
    .bind(&prefix)
    .bind(role.to_string())
    .bind(request.rate_limit)
    .bind(request.daily_limit)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(created_by)
    .bind(&request.notes)
    .bind(key_plaintext)
    .execute(pool)
    .await?;

    Ok(ApiKeyCreated {
        id,
        name: request.name.clone(),
        api_key: key,
        key_prefix: prefix,
        role,
        rate_limit: request.rate_limit,
        daily_limit: request.daily_limit,
        created_at: now,
    })
}

/// Trova API Key per hash
#[allow(clippy::type_complexity)]
pub async fn find_by_key(pool: &DbPool, api_key: &str) -> Result<Option<ApiKey>, sqlx::Error> {
    let hash = hash_api_key(api_key);

    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        Option<i64>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT id, name, key_hash, key_prefix, role, is_active, rate_limit, daily_limit,
               created_at, updated_at, last_used_at, created_by, notes
        FROM api_keys
        WHERE key_hash = ?
        "#,
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((
            id,
            name,
            key_hash,
            key_prefix,
            role,
            is_active,
            rate_limit,
            daily_limit,
            created_at,
            updated_at,
            last_used_at,
            created_by,
            notes,
        )) => Ok(Some(ApiKey {
            id,
            name,
            key_hash,
            key_prefix,
            role: ApiKeyRole::from(role.as_str()),
            is_active: is_active != 0,
            rate_limit,
            daily_limit,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_used_at: last_used_at.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            }),
            created_by,
            notes,
        })),
        None => Ok(None),
    }
}

/// Lista tutte le API Keys
#[allow(clippy::type_complexity)]
pub async fn list_all(pool: &DbPool) -> Result<Vec<ApiKey>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        Option<i64>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT id, name, key_hash, key_prefix, role, is_active, rate_limit, daily_limit,
               created_at, updated_at, last_used_at, created_by, notes
        FROM api_keys
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                name,
                key_hash,
                key_prefix,
                role,
                is_active,
                rate_limit,
                daily_limit,
                created_at,
                updated_at,
                last_used_at,
                created_by,
                notes,
            )| {
                ApiKey {
                    id,
                    name,
                    key_hash,
                    key_prefix,
                    role: ApiKeyRole::from(role.as_str()),
                    is_active: is_active != 0,
                    rate_limit,
                    daily_limit,
                    created_at: DateTime::parse_from_rfc3339(&created_at)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    last_used_at: last_used_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    }),
                    created_by,
                    notes,
                }
            },
        )
        .collect())
}

/// Aggiorna timestamp ultimo uso
pub async fn update_last_used(pool: &DbPool, api_key_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE api_keys SET last_used_at = ? WHERE id = ?
        "#,
    )
    .bind(Utc::now().to_rfc3339())
    .bind(api_key_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Aggiorna API Key
pub async fn update_api_key(
    pool: &DbPool,
    id: &str,
    request: &UpdateApiKeyRequest,
) -> Result<bool, sqlx::Error> {
    let mut updates = Vec::new();
    let mut values: Vec<String> = Vec::new();

    if let Some(ref name) = request.name {
        updates.push("name = ?");
        values.push(name.clone());
    }
    if let Some(is_active) = request.is_active {
        updates.push("is_active = ?");
        values.push(if is_active {
            "1".to_string()
        } else {
            "0".to_string()
        });
    }
    if let Some(rate_limit) = request.rate_limit {
        updates.push("rate_limit = ?");
        values.push(rate_limit.to_string());
    }
    if let Some(ref notes) = request.notes {
        updates.push("notes = ?");
        values.push(notes.clone());
    }

    if updates.is_empty() {
        return Ok(false);
    }

    updates.push("updated_at = ?");
    values.push(Utc::now().to_rfc3339());

    let query = format!("UPDATE api_keys SET {} WHERE id = ?", updates.join(", "));

    let mut q = sqlx::query(&query);
    for value in &values {
        q = q.bind(value);
    }
    q = q.bind(id);

    let result = q.execute(pool).await?;
    Ok(result.rows_affected() > 0)
}

/// Elimina API Key
pub async fn delete_api_key(pool: &DbPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM api_keys WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Verifica se esiste almeno un admin
pub async fn has_admin(pool: &DbPool) -> Result<bool, sqlx::Error> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM api_keys WHERE role = 'admin' AND is_active = 1")
            .fetch_one(pool)
            .await?;
    Ok(count.0 > 0)
}

/// Crea il primo admin se non esiste
pub async fn ensure_initial_admin(pool: &DbPool) -> Result<Option<ApiKeyCreated>, sqlx::Error> {
    if has_admin(pool).await? {
        return Ok(None);
    }

    let request = CreateApiKeyRequest {
        name: "Initial Admin".to_string(),
        role: "admin".to_string(),
        rate_limit: 1000,
        daily_limit: None,
        notes: Some("Chiave admin iniziale creata automaticamente".to_string()),
    };

    let key = create_api_key(pool, &request, None).await?;
    Ok(Some(key))
}

/// Recupera la chiave API in chiaro per un utente (solo per ruolo "user", non admin)
pub async fn get_plaintext_key(
    pool: &DbPool,
    api_key_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT key_plaintext FROM api_keys WHERE id = ? AND role = 'user'")
            .bind(api_key_id)
            .fetch_optional(pool)
            .await?;

    Ok(row.and_then(|(plaintext,)| plaintext))
}
