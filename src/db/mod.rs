pub mod api_keys;
pub mod jobs;
#[cfg(feature = "google-auth")]
pub mod oauth_users;
pub mod stats;
#[cfg(feature = "google-auth")]
pub mod user_settings;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::time::Duration;

pub type DbPool = SqlitePool;

/// Inizializza il database SQLite
pub async fn init_db(database_url: &str) -> Result<DbPool, sqlx::Error> {
    // Crea il pool di connessioni
    let pool = SqlitePoolOptions::new()
        .max_connections(20)
        .idle_timeout(Duration::from_secs(60))
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;

    // Esegui le migrazioni
    run_migrations(&pool).await?;

    Ok(pool)
}

/// Esegue le migrazioni del database
async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    // Crea tabella API Keys
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            key_prefix TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'user',
            is_active INTEGER NOT NULL DEFAULT 1,
            rate_limit INTEGER NOT NULL DEFAULT 100,
            daily_limit INTEGER,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_used_at TEXT,
            created_by TEXT,
            notes TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Crea tabella statistiche conversioni
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversion_records (
            id TEXT PRIMARY KEY,
            timestamp TEXT NOT NULL,
            api_key_id TEXT,
            is_guest INTEGER NOT NULL DEFAULT 0,
            conversion_type TEXT NOT NULL,
            input_format TEXT NOT NULL,
            output_format TEXT NOT NULL,
            input_size_bytes INTEGER NOT NULL,
            output_size_bytes INTEGER NOT NULL,
            processing_time_ms INTEGER NOT NULL,
            success INTEGER NOT NULL,
            error TEXT,
            client_ip TEXT,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Crea indici per performance
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_conversion_timestamp ON conversion_records(timestamp);
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_conversion_api_key ON conversion_records(api_key_id);
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
        "#,
    )
    .execute(pool)
    .await?;

    // Crea tabella configurazione guest
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS guest_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            enabled INTEGER NOT NULL DEFAULT 1,
            rate_limit_per_minute INTEGER NOT NULL DEFAULT 10,
            daily_limit INTEGER NOT NULL DEFAULT 50,
            max_file_size_mb INTEGER NOT NULL DEFAULT 5,
            allowed_types TEXT NOT NULL DEFAULT 'image',
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Inserisci configurazione guest di default se non esiste
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO guest_config (id, enabled, rate_limit_per_minute, daily_limit, max_file_size_mb, allowed_types, updated_at)
        VALUES (1, 1, 10, 50, 5, 'image', datetime('now'))
        "#,
    )
    .execute(pool)
    .await?;

    // Crea tabella uso giornaliero guest (per IP)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS guest_daily_usage (
            ip_address TEXT NOT NULL,
            date TEXT NOT NULL,
            conversions INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (ip_address, date)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Crea tabella jobs per persistenza
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            api_key_id TEXT,
            conversion_type TEXT NOT NULL,
            input_format TEXT NOT NULL,
            output_format TEXT NOT NULL,
            quality INTEGER,
            status TEXT NOT NULL DEFAULT 'pending',
            progress INTEGER NOT NULL DEFAULT 0,
            progress_message TEXT,
            input_path TEXT NOT NULL,
            result_path TEXT,
            error TEXT,
            file_size_bytes INTEGER,
            created_at TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Indici per jobs
    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_jobs_api_key ON jobs(api_key_id)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_jobs_created_at ON jobs(created_at)"#,
    )
    .execute(pool)
    .await?;

    // Aggiungi colonne per limiti job a api_keys (ignora errore se esistono già)
    let _ = sqlx::query(
        r#"ALTER TABLE api_keys ADD COLUMN max_concurrent_jobs INTEGER DEFAULT 5"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE api_keys ADD COLUMN job_timeout_seconds INTEGER DEFAULT 300"#,
    )
    .execute(pool)
    .await;

    // Nuove colonne per jobs: priority, webhook, source_url, expires_at, retry_count
    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN priority TEXT DEFAULT 'normal'"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN webhook_url TEXT"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN source_url TEXT"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN expires_at TEXT"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN retry_count INTEGER DEFAULT 0"#,
    )
    .execute(pool)
    .await;

    // Indice per priority (job prioritari elaborati prima)
    let _ = sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_jobs_priority ON jobs(priority DESC, created_at ASC)"#,
    )
    .execute(pool)
    .await;

    // Indice per expires_at (per cleanup)
    let _ = sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_jobs_expires_at ON jobs(expires_at)"#,
    )
    .execute(pool)
    .await;

    // Crea tabella OAuth users (Google login)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS oauth_users (
            id TEXT PRIMARY KEY,
            google_id TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL,
            name TEXT,
            picture_url TEXT,
            api_key_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_login_at TEXT NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Indici per oauth_users
    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_oauth_users_google_id ON oauth_users(google_id)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_oauth_users_email ON oauth_users(email)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_oauth_users_api_key ON oauth_users(api_key_id)"#,
    )
    .execute(pool)
    .await?;

    // Aggiungi colonna key_plaintext per utenti non-admin (ignora errore se esiste)
    // Admin keys restano hashate, user keys salvate in chiaro per poterle recuperare
    let _ = sqlx::query(
        r#"ALTER TABLE api_keys ADD COLUMN key_plaintext TEXT"#,
    )
    .execute(pool)
    .await;

    // Aggiungi original_filename alla tabella jobs
    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN original_filename TEXT"#,
    )
    .execute(pool)
    .await;

    // Aggiungi colonne token OAuth alla tabella oauth_users
    let _ = sqlx::query(
        r#"ALTER TABLE oauth_users ADD COLUMN access_token TEXT"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE oauth_users ADD COLUMN refresh_token TEXT"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"ALTER TABLE oauth_users ADD COLUMN token_expires_at TEXT"#,
    )
    .execute(pool)
    .await;

    // Tabella user_settings per preferenze utente
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_settings (
            user_id TEXT PRIMARY KEY,
            save_to_drive_enabled INTEGER NOT NULL DEFAULT 0,
            drive_folder_id TEXT,
            drive_folder_name TEXT DEFAULT 'Converty Exports',
            auto_save_original_filename INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (user_id) REFERENCES oauth_users(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Aggiungi drive_file_id alla tabella jobs per tracking upload Drive
    let _ = sqlx::query(
        r#"ALTER TABLE jobs ADD COLUMN drive_file_id TEXT"#,
    )
    .execute(pool)
    .await;

    // Aggiungi drive_filter_types a user_settings per filtrare quali tipi salvare su Drive
    // Valori: "all" o lista separata da virgole es. "image,audio,video,document"
    let _ = sqlx::query(
        r#"ALTER TABLE user_settings ADD COLUMN drive_filter_types TEXT DEFAULT 'all'"#,
    )
    .execute(pool)
    .await;

    Ok(())
}
