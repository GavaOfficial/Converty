use std::net::SocketAddr;

use axum::{middleware, Router};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use converty::config::Config;
use converty::db;
use converty::db::api_keys::{
    self, ApiKey, ApiKeyCreated, ApiKeyRole, CreateApiKeyRequest, UpdateApiKeyRequest,
};
use converty::db::jobs::{JobRecord, JobsListResponse, JobsQuery};
use converty::db::stats::GuestConfig;
use converty::middleware::auth::{self, AuthState};
use converty::middleware::rate_limit;
use converty::models::{JobPriority, *};
use converty::routes;
use converty::routes::admin::{ApiKeyWithStats, CleanupRequest, CleanupResponse, MessageResponse};
use converty::routes::auth::{
    CurrentUserResponse, GoogleAuthUrlResponse, UserInfo, UserStats as AuthUserStats,
};
use converty::services::queue;
use converty::utils::check_ffmpeg_available;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Converty API",
        version = "1.0.0",
        description = "API per la conversione di file multimediali (immagini, documenti, audio, video)",
        license(name = "MIT"),
    ),
    paths(
        crate::routes::convert::convert_image,
        crate::routes::convert::convert_document,
        crate::routes::convert::convert_audio,
        crate::routes::convert::convert_video,
        crate::routes::convert::convert_batch,
        crate::routes::health::health_check,
        crate::routes::health::get_formats,
        crate::routes::stats::get_stats,
        crate::routes::stats::get_summary,
        crate::routes::jobs::list_jobs,
        crate::routes::jobs::create_job,
        crate::routes::jobs::get_job_status,
        crate::routes::jobs::delete_job,
        crate::routes::jobs::download_job_result,
        crate::routes::jobs::job_progress_stream,
        crate::routes::jobs::retry_job,
        crate::routes::jobs::cancel_job,
        crate::routes::admin::list_api_keys,
        crate::routes::admin::create_api_key,
        crate::routes::admin::get_api_key,
        crate::routes::admin::update_api_key,
        crate::routes::admin::delete_api_key,
        crate::routes::admin::get_guest_config,
        crate::routes::admin::update_guest_config,
        crate::routes::admin::cleanup_old_data,
        crate::routes::auth::get_google_auth_url,
        crate::routes::auth::google_callback,
        crate::routes::auth::get_current_user,
    ),
    components(schemas(
        HealthResponse,
        FormatsResponse,
        FormatSupport,
        BatchConvertResponse,
        ConvertedFile,
        FailedFile,
        JobResponse,
        JobCreatedResponse,
        JobStatus,
        ConversionType,
        ErrorResponse,
        StatsResponse,
        GlobalStats,
        ApiKeyStats,
        TypeStats,
        FormatStats,
        TimeWindowStats,
        StatsSummary,
        ProgressUpdate,
        ApiKey,
        ApiKeyCreated,
        ApiKeyRole,
        CreateApiKeyRequest,
        UpdateApiKeyRequest,
        ApiKeyWithStats,
        GuestConfig,
        CleanupRequest,
        CleanupResponse,
        MessageResponse,
        JobRecord,
        JobsListResponse,
        JobsQuery,
        JobPriority,
        GoogleAuthUrlResponse,
        CurrentUserResponse,
        UserInfo,
        AuthUserStats,
    )),
    tags(
        (name = "Conversione", description = "Endpoints per convertire file"),
        (name = "Sistema", description = "Health check e info"),
        (name = "Jobs", description = "Gestione job asincroni"),
        (name = "Statistiche", description = "Statistiche conversioni"),
        (name = "Admin", description = "Gestione API Keys e configurazione"),
        (name = "Auth", description = "Autenticazione Google OAuth"),
    ),
    servers(
        (url = "http://localhost:4000", description = "Server locale"),
    ),
    security(
        ("api_key" = [])
    ),
    modifiers(&SecurityAddon)
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                utoipa::openapi::security::SecurityScheme::ApiKey(
                    utoipa::openapi::security::ApiKey::Header(
                        utoipa::openapi::security::ApiKeyValue::new("X-API-Key"),
                    ),
                ),
            );
        }
    }
}

#[tokio::main]
async fn main() {
    // Carica variabili da .env
    dotenvy::dotenv().ok();

    // Inizializza logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "converty=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Carica configurazione
    let config = Config::from_env();

    // Inizializza database SQLite
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:converty.db?mode=rwc".to_string());

    tracing::info!("Connessione al database: {}", database_url);

    let db_pool = match db::init_db(&database_url).await {
        Ok(pool) => {
            tracing::info!("Database SQLite inizializzato");
            pool
        }
        Err(e) => {
            tracing::error!("Errore inizializzazione database: {}", e);
            std::process::exit(1);
        }
    };

    // Crea admin iniziale se non esiste
    match api_keys::ensure_initial_admin(&db_pool).await {
        Ok(Some(admin_key)) => {
            tracing::warn!("========================================");
            tracing::warn!("  CHIAVE ADMIN INIZIALE CREATA!");
            tracing::warn!("========================================");
            tracing::warn!("  API Key: {}", admin_key.api_key);
            tracing::warn!("  SALVA QUESTA CHIAVE - NON SARA' PIU' MOSTRATA!");
            tracing::warn!("========================================");
        }
        Ok(None) => {
            tracing::info!("Admin esistente trovato");
        }
        Err(e) => {
            tracing::error!("Errore creazione admin: {}", e);
        }
    }

    // Verifica FFmpeg
    if check_ffmpeg_available() {
        tracing::info!("FFmpeg disponibile - conversione audio/video abilitata");
    } else {
        tracing::warn!("FFmpeg non trovato - conversione audio/video disabilitata");
    }

    // Crea rate limiter (100 richieste/minuto per default)
    let rate_limiter = rate_limit::create_rate_limiter(100);

    // Crea job queue con broadcast channel per progress
    let (job_queue, progress_tx) = queue::create_job_queue(db_pool.clone());

    // Crea directory temporanea
    std::fs::create_dir_all(&config.temp_dir).ok();

    // CORS layer - espone Content-Disposition per il download
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers([
            axum::http::header::CONTENT_DISPOSITION,
            axum::http::header::CONTENT_TYPE,
        ]);

    // Auth state per middleware
    let auth_state = AuthState {
        db: db_pool.clone(),
    };

    // API routes con middleware
    let api_routes = routes::create_router(
        job_queue,
        progress_tx,
        db_pool.clone(),
        config.clone(),
        config.google_client_id.clone(),
        config.google_client_secret.clone(),
        config.frontend_url.clone(),
    )
    .layer(middleware::from_fn_with_state(
        auth_state,
        auth::api_key_auth,
    ))
    .layer(middleware::from_fn(move |req, next| {
        let limiter = rate_limiter.clone();
        async move { rate_limit::rate_limit_middleware(limiter, req, next).await }
    }));

    // Costruisci router completo con Swagger
    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(api_routes)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .into_make_service_with_connect_info::<SocketAddr>();

    // Avvia server
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Indirizzo non valido");

    tracing::info!("========================================");
    tracing::info!("  Converty API v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("========================================");
    tracing::info!("Server: http://{}", addr);
    tracing::info!("Swagger UI: http://{}/swagger-ui/", addr);
    tracing::info!("----------------------------------------");
    tracing::info!("Modalita':");
    tracing::info!("  - Guest: Limitato (config. da admin)");
    tracing::info!("  - User:  API Key standard");
    tracing::info!("  - Admin: Gestione completa");
    tracing::info!("----------------------------------------");
    tracing::info!("Endpoints Pubblici:");
    tracing::info!("  GET  /api/v1/health           - Health check");
    tracing::info!("  GET  /api/v1/formats          - Formati supportati");
    tracing::info!("  GET  /api/v1/stats/summary    - Statistiche pubbliche");
    tracing::info!("----------------------------------------");
    tracing::info!("Endpoints Conversione:");
    tracing::info!("  POST /api/v1/convert/image    - Converti immagine");
    tracing::info!("  POST /api/v1/convert/document - Converti documento");
    tracing::info!("  POST /api/v1/convert/audio    - Converti audio");
    tracing::info!("  POST /api/v1/convert/video    - Converti video");
    tracing::info!("  POST /api/v1/convert/batch    - Batch (no guest)");
    tracing::info!("----------------------------------------");
    tracing::info!("Endpoints Jobs (Asincroni):");
    tracing::info!("  GET  /api/v1/jobs             - Lista tutti i job");
    tracing::info!("  POST /api/v1/jobs             - Crea job");
    tracing::info!("  GET  /api/v1/jobs/:id         - Stato job");
    tracing::info!("  GET  /api/v1/jobs/:id/progress- SSE progress stream");
    tracing::info!("  GET  /api/v1/jobs/:id/download- Scarica risultato");
    tracing::info!("  DEL  /api/v1/jobs/:id         - Elimina job");
    tracing::info!("----------------------------------------");
    tracing::info!("Endpoints Admin:");
    tracing::info!("  GET  /api/v1/admin/keys       - Lista API Keys");
    tracing::info!("  POST /api/v1/admin/keys       - Crea API Key");
    tracing::info!("  PUT  /api/v1/admin/keys/:id   - Modifica API Key");
    tracing::info!("  DEL  /api/v1/admin/keys/:id   - Elimina API Key");
    tracing::info!("  GET  /api/v1/admin/guest      - Config guest");
    tracing::info!("  PUT  /api/v1/admin/guest      - Modifica guest");
    tracing::info!("  POST /api/v1/admin/cleanup    - Pulisci vecchi dati");
    tracing::info!("----------------------------------------");
    tracing::info!("Endpoints Auth:");
    tracing::info!("  POST /api/v1/auth/google      - Login con Google");
    tracing::info!("  GET  /api/v1/auth/me          - Info utente corrente");
    tracing::info!("----------------------------------------");
    if config.google_client_id.is_some() {
        tracing::info!("Google OAuth: Configurato");
    } else {
        tracing::warn!("Google OAuth: NON configurato (imposta GOOGLE_CLIENT_ID)");
    }

    // Task background per cleanup job vecchi (ogni ora)
    let cleanup_pool = db_pool.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            tracing::info!("Avvio cleanup job vecchi...");
            match converty::db::jobs::cleanup_old_jobs(&cleanup_pool, 7).await {
                Ok((count, files)) => {
                    tracing::info!(
                        "Cleanup completato: {} job eliminati, {} file da rimuovere",
                        count,
                        files.len()
                    );
                    for file in files {
                        if let Err(e) = std::fs::remove_file(&file) {
                            tracing::warn!("Errore rimozione file {}: {}", file, e);
                        }
                    }
                }
                Err(e) => tracing::error!("Errore cleanup: {}", e),
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
