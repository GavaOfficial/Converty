//! Authentication-related models

use crate::db::api_keys::ApiKeyRole;

/// Authenticated user information extracted from request
#[derive(Clone, Debug)]
pub struct AuthInfo {
    /// API key ID if authenticated with an API key
    pub api_key_id: Option<String>,
    /// Whether this is a guest (unauthenticated) request
    pub is_guest: bool,
    /// Role of the authenticated user
    pub role: ApiKeyRole,
    /// Client IP address
    pub client_ip: Option<String>,
}

impl Default for AuthInfo {
    fn default() -> Self {
        Self {
            api_key_id: None,
            is_guest: true,
            role: ApiKeyRole::User,
            client_ip: None,
        }
    }
}
