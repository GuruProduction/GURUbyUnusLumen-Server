//! GURU open server database layer.
//!
//! Provides async PostgreSQL connectivity via sqlx, an embedded migration
//! runner, and typed repositories for the content catalog. There are no user
//! tables in this layer — that is a permanent design property, not a TODO.

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
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
///
/// You cannot connect to a database that does not exist, so the existence
/// check and CREATE both run through a single-connection pool to the
/// "postgres" maintenance database on the same server. The database name is
/// bound as a parameter everywhere it touches SQL.
pub async fn create_database_if_missing(database_url: &str) -> Result<(), sqlx::Error> {
    let db_name = database_name(database_url)?;
    let admin_url = admin_url_for(database_url);

    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_url)
        .await?;

    let exists: bool = sqlx::query("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
        .bind(&db_name)
        .fetch_one(&admin)
        .await?
        .get::<bool, _>(0);

    if exists {
        info!(db = %db_name, "database already exists");
        return Ok(());
    }

    // Identifier for CREATE DATABASE cannot be parameter-bound; validate the
    // name shape before interpolating it.
    if db_name.is_empty()
        || !db_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        || db_name.starts_with(|c: char| c.is_ascii_digit())
    {
        return Err(sqlx::Error::Configuration(
            format!("unsafe database name: {db_name}").into(),
        ));
    }

    info!(db = %db_name, "creating database");
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await?;
    admin.close().await;
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

/// postgresql://user@host:port/target -> postgresql://user@host:port/postgres
fn admin_url_for(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => format!("{}/postgres", &trimmed[..i]),
        None => format!("{trimmed}/postgres"),
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