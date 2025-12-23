use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use serde_json::json;
use std::num::NonZeroU32;
use std::sync::Arc;

pub type SharedRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// Crea un rate limiter con il limite specificato di richieste al minuto
pub fn create_rate_limiter(requests_per_minute: u32) -> SharedRateLimiter {
    let quota = Quota::per_minute(NonZeroU32::new(requests_per_minute).unwrap());
    Arc::new(RateLimiter::direct(quota))
}

/// Middleware per rate limiting
pub async fn rate_limit_middleware(
    limiter: SharedRateLimiter,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    match limiter.check() {
        Ok(_) => Ok(next.run(request).await),
        Err(_) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Troppe richieste. Riprova tra poco.",
                "status": 429
            })),
        )),
    }
}
