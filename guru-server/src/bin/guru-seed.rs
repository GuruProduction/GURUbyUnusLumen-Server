//! guru-seed: idempotent bootstrap for the GURU open server database.
//!
//! Creates the database if missing, runs migrations, seeds the 21 built-in
//! mask templates into the agents catalog, and issues a reviewer token when
//! one does not exist yet. Safe to run repeatedly: every step is upsert-only
//! except token issuance, which checks existence first.
//!
//! Usage:
//!   DATABASE_URL=... guru-seed                  # seed + print reviewer token
//!   DATABASE_URL=... guru-seed --token LABEL    # issue an additional token

use guru_db::repositories::ContentRepository;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| guru_db::DEFAULT_DATABASE_URL.to_string());

    let extra_token = std::env::args().nth(1);
    let token_label = std::env::args().nth(2);

    tracing_subscriber::fmt()
        .with_env_filter("guru_seed=info")
        .init();

    let pool = guru_db::setup_database(&database_url).await?;
    let content = ContentRepository::new(pool.clone());
    let reviews = guru_db::repositories::ReviewRepository::new(pool);

    // -- seed the 21 built-in masks ------------------------------------------
    for mask in guru_content::built_in_masks() {
        let permissions: serde_json::Value =
            serde_json::to_value(&mask.tool_permissions).expect("permissions serialize");
        content
            .insert_agent(
                mask.id,
                &mask.slug,
                &mask.name,
                &mask.role_description,
                &mask.system_prompt,
                &permissions,
                None,
            )
            .await
            .map_err(|e| format!("seeding mask {} failed: {e}", mask.slug))?;
        tracing::info!(slug = %mask.slug, wave = mask.wave, "seeded built-in mask");
    }

    // -- reviewer token --------------------------------------------------------
    let label = token_label.as_deref().unwrap_or("initial-reviewer");
    let token_value = format!("guru-rev-{}", Uuid::new_v4().simple());
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(token_value.as_bytes());
        hex::encode(hasher.finalize())
    };

    if let Some(_existing) = reviews.verify_review_token(&digest).await? {
        tracing::info!(%label, "token already exists");
    } else {
        reviews.create_review_token(label, &digest).await?;
        println!("\nREVIEWER TOKEN (store now, shown once):\n  {token_value}\n");
    }

    if extra_token.as_deref() == Some("--token") {
        let value = format!("guru-rev-{}", Uuid::new_v4().simple());
        let digest = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(value.as_bytes());
            hex::encode(hasher.finalize())
        };
        reviews
            .create_review_token(token_label.as_deref().unwrap_or("reviewer"), &digest)
            .await?;
        println!("\nADDITIONAL TOKEN:\n  {value}\n");
    }

    println!("seed complete: 21 built-in masks live, review tokens ready");
    Ok(())
}