//! GURU open server binary.
//!
//! The publisher, not the collector. Public content API + community review
//! pipeline. Stateless by design: no accounts, no conversation capture, no
//! PII. Run your own, or point GURU at ours.

mod etag;
mod public;
mod review;
mod state;

use state::AppState;
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "guru_server=info,tower_http=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| guru_db::DEFAULT_DATABASE_URL.to_string());
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:4400".into())
        .parse()?;
    let content_dir = std::env::var("CONTENT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("content")
        });

    let pool = guru_db::setup_database(&database_url).await?;
    tokio::fs::create_dir_all(content_dir.join("_staging")).await?;

    let state = AppState {
        content: guru_db::repositories::ContentRepository::new(pool.clone()),
        reviews: guru_db::repositories::ReviewRepository::new(pool),
        content_dir,
    };

    let app = public::public_router()
        .merge(review::review_router())
        .route("/health", axum::routing::get(health))
        .with_state(state);

    info!(%bind_addr, "starting GURU open server");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "server": "guru-open-server",
        "design": "publisher-not-collector",
    }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}