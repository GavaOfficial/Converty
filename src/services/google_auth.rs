use jsonwebtoken::{decode, decode_header, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Claims del token Google ID
#[derive(Debug, Deserialize)]
pub struct GoogleClaims {
    /// Google user ID (subject)
    pub sub: String,
    /// Email dell'utente
    pub email: String,
    /// Email verificata
    #[serde(default)]
    pub email_verified: bool,
    /// Nome completo
    pub name: Option<String>,
    /// URL foto profilo
    pub picture: Option<String>,
    /// Audience (deve corrispondere al Client ID)
    pub aud: String,
    /// Issuer (deve essere accounts.google.com)
    pub iss: String,
    /// Expiration time
    pub exp: usize,
    /// Issued at
    pub iat: usize,
}

/// Errori di autenticazione Google
#[derive(Debug, thiserror::Error)]
pub enum GoogleAuthError {
    #[error("Token non valido: {0}")]
    InvalidToken(String),
    #[error("Token scaduto")]
    TokenExpired,
    #[error("Issuer non valido")]
    InvalidIssuer,
    #[error("Audience non valido")]
    InvalidAudience,
    #[error("Errore nel recupero delle chiavi Google: {0}")]
    KeyFetchError(String),
    #[error("Chiave non trovata: {0}")]
    KeyNotFound(String),
}

/// Chiave pubblica Google (JWK)
#[derive(Debug, Deserialize, Clone)]
pub struct GoogleJwk {
    pub kid: String,
    pub n: String,
    pub e: String,
    pub kty: String,
    pub alg: String,
}

/// Risposta delle chiavi Google
#[derive(Debug, Deserialize)]
pub struct GoogleJwks {
    pub keys: Vec<GoogleJwk>,
}

/// Cache per le chiavi pubbliche di Google
pub struct GoogleKeysCache {
    keys: RwLock<Option<(HashMap<String, GoogleJwk>, Instant)>>,
    cache_duration: Duration,
}

impl GoogleKeysCache {
    pub fn new() -> Self {
        Self {
            keys: RwLock::new(None),
            cache_duration: Duration::from_secs(3600), // 1 ora
        }
    }

    /// Ottiene le chiavi, fetchandole se necessario
    pub async fn get_keys(&self) -> Result<HashMap<String, GoogleJwk>, GoogleAuthError> {
        // Controlla cache
        {
            let cache = self.keys.read().unwrap();
            if let Some((keys, fetched_at)) = cache.as_ref() {
                if fetched_at.elapsed() < self.cache_duration {
                    return Ok(keys.clone());
                }
            }
        }

        // Fetch nuove chiavi
        let keys = fetch_google_keys().await?;

        // Aggiorna cache
        {
            let mut cache = self.keys.write().unwrap();
            *cache = Some((keys.clone(), Instant::now()));
        }

        Ok(keys)
    }
}

impl Default for GoogleKeysCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Fetch delle chiavi pubbliche di Google
async fn fetch_google_keys() -> Result<HashMap<String, GoogleJwk>, GoogleAuthError> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://www.googleapis.com/oauth2/v3/certs")
        .send()
        .await
        .map_err(|e| GoogleAuthError::KeyFetchError(e.to_string()))?;

    let jwks: GoogleJwks = response
        .json()
        .await
        .map_err(|e| GoogleAuthError::KeyFetchError(e.to_string()))?;

    let mut keys = HashMap::new();
    for key in jwks.keys {
        keys.insert(key.kid.clone(), key);
    }

    Ok(keys)
}

/// Verifica un token Google ID
pub async fn verify_google_token(
    id_token: &str,
    client_id: &str,
    keys_cache: &GoogleKeysCache,
) -> Result<GoogleClaims, GoogleAuthError> {
    // Decodifica l'header per ottenere il kid
    let header = decode_header(id_token)
        .map_err(|e| GoogleAuthError::InvalidToken(e.to_string()))?;

    let kid = header.kid.ok_or_else(|| {
        GoogleAuthError::InvalidToken("Token senza kid nell'header".to_string())
    })?;

    // Ottieni le chiavi pubbliche
    let keys = keys_cache.get_keys().await?;

    // Trova la chiave corrispondente
    let jwk = keys.get(&kid).ok_or_else(|| {
        GoogleAuthError::KeyNotFound(kid.clone())
    })?;

    // Costruisci la chiave di decodifica
    let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
        .map_err(|e| GoogleAuthError::InvalidToken(e.to_string()))?;

    // Configura la validazione
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

    // Decodifica e valida il token
    let token_data = decode::<GoogleClaims>(id_token, &decoding_key, &validation)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => GoogleAuthError::TokenExpired,
            jsonwebtoken::errors::ErrorKind::InvalidIssuer => GoogleAuthError::InvalidIssuer,
            jsonwebtoken::errors::ErrorKind::InvalidAudience => GoogleAuthError::InvalidAudience,
            _ => GoogleAuthError::InvalidToken(e.to_string()),
        })?;

    Ok(token_data.claims)
}

/// Risultato semplificato per le route
#[derive(Debug, Serialize)]
pub struct VerifiedGoogleUser {
    pub google_id: String,
    pub email: String,
    pub name: Option<String>,
    pub picture_url: Option<String>,
}

impl From<GoogleClaims> for VerifiedGoogleUser {
    fn from(claims: GoogleClaims) -> Self {
        Self {
            google_id: claims.sub,
            email: claims.email,
            name: claims.name,
            picture_url: claims.picture,
        }
    }
}
