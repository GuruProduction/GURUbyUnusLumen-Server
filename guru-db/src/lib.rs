//! GURU open server database layer.
//!
//! Provides async PostgreSQL connectivity via sqlx, an embedded migration
//! runner, and typed repositories for the content catalog. There are no user
//! tables in this layer — that is a permanent design property, not a TODO.

use sqlx::{migrate::MigrateDatabase, postgres::PgPoolOptions, PgPool, Postgres};
use std::time::Duration;
use tracing::{info, warn};

pub mod models;
pub mod repositories;

/// Default local dev database. A brand-new, separate database — never the
/// legacy guru_dev that the live Docker stack runs on.
pub const DEFAULT_DATABASE_URL: &str = "postgresql://unuslumen@localhost:5432/guru_server";

/// Run the embedded migrations.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    info!("running embedded sqlx migrations");
    sqlx::migrate!("./src/migrations").run(pool).await
}

/// Create the target database if it does not already exist.
pub async fn create_database_if_missing(database_url: &str) -> Result<(), sqlx::Error> {
    let url = connection_url_without_db(database_url);
    let db_name = database_name(database_url)?;

    if !Postgres::database_exists(&url).await.unwrap_or(false) {
        info!(db = %db_name, "creating database");
        Postgres::create_database(&url).await?;
    } else {
        info!(db = %db_name, "database already exists");
    }
    Ok(())
}

/// Build a connection pool for the application.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(16)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .connect(database_url)
        .await
}

/// Ensure DB exists, connect, and migrate.
pub async fn setup_database(database_url: &str) -> Result<PgPool, sqlx::Error> {
    create_database_if_missing(database_url).await?;
    let pool = connect(database_url).await?;
    migrate(&pool).await.map_err(|e| {
        warn!(error = %e, "migration failure");
        sqlx::Error::Migrate(Box::new(e))
    })?;
    Ok(pool)
}

fn connection_url_without_db(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..i].to_string(),
        None => url.to_string(),
    }
}

fn database_name(url: &str) -> Result<String, sqlx::Error> {
    let trimmed = url.trim_end_matches('?');
    trimmed
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            sqlx::Error::Configuration(format!("cannot parse database name from {url}").into())
        })
}