//! guru-import-prompts: pull master prompts from the LEGACY guru_dev database
//! into the new open server. The legacy DB is opened READ-ONLY — this process
//! never issues a single write against it. Only INSERTs go to the new DB.
//!
//! Usage:
//!   SOURCE_URL=postgresql://unuslumen@localhost:5432/guru_dev guru-import-prompts

use guru_db::repositories::ContentRepository;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let source_url = std::env::var("SOURCE_URL")
        .unwrap_or_else(|_| "postgresql://unuslumen@localhost:5432/guru_dev".into());
    let target_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| guru_db::DEFAULT_DATABASE_URL.to_string());

    tracing_subscriber::fmt().with_env_filter("guru_import=info").init();

    let source = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&source_url)
        .await?;
    // Hard read-only belt over the source DB: fail the run immediately if a
    // write ever slipped through.
    sqlx::query("SET default_transaction_read_only = on")
        .execute(&source)
        .await?;

    let legacy: Vec<LegacyPrompt> = sqlx::query_as(
        "SELECT name, category, content, version, is_active, delivery_mode, trigger_keywords, match_threshold \
         FROM master_prompts ORDER BY category, name",
    )
    .fetch_all(&source)
    .await?;
    source.close().await;

    let pool = guru_db::setup_database(&target_url).await?;
    let content = ContentRepository::new(pool);

    let mut imported = 0;
    for p in &legacy {
        let slug = slugify(&p.name);
        let delivery = if p.delivery_mode == "keyword_injection" {
            "keyword_injection"
        } else {
            "always"
        };
        let row = content
            .upsert_prompt(
                uuid::Uuid::new_v4(),
                &slug,
                &p.category,
                &p.name,
                &p.content,
                delivery,
                &p.trigger_keywords,
                p.match_threshold,
            )
            .await
            .map_err(|e| format!("import {} failed: {e}", p.name))?;
        if !p.is_active {
            content.set_prompt_active(&row.slug, false).await?;
        }
        imported += 1;
        tracing::info!(slug = %row.slug, category = %p.category, v = row.version, "imported");
    }

    println!("import complete: {imported} prompt sections from the legacy DB");
    Ok(())
}

#[derive(sqlx::FromRow)]
struct LegacyPrompt {
    name: String,
    category: String,
    content: String,
    #[allow(dead_code)]
    version: i32,
    is_active: bool,
    delivery_mode: String,
    trigger_keywords: Vec<String>,
    match_threshold: Option<f64>,
}

fn slugify(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else if c.is_whitespace() || c == '-' || c == '_' || c == '/' {
                '-'
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('-').to_string();
    let mut deduped = String::new();
    let mut prev_dash = false;
    for ch in trimmed.chars() {
        if ch == '-' {
            if !prev_dash {
                deduped.push('-');
            }
            prev_dash = true;
        } else {
            deduped.push(ch);
            prev_dash = false;
        }
    }
    if deduped.is_empty() {
        "unnamed-section".into()
    } else {
        deduped.chars().take(80).collect()
    }
}