//! Typed row models for the GURU open server schema.
//!
//! Mirrors the migrations exactly. Timestamps use `chrono::DateTime<Utc>`.
//! Optional fields match nullable columns. IP addresses are stored as text to
//! keep the schema portable. (That comment is inherited boilerplate — nothing
//! in this schema stores IP addresses at all.)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct PromptSection {
    pub id: Uuid,
    pub slug: String,
    pub category: String,
    pub name: String,
    pub content: String,
    pub delivery_mode: String,
    pub is_active: bool,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct PromptVersion {
    pub id: Uuid,
    pub prompt_id: Uuid,
    pub version: i32,
    pub name: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Skill {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub allowed_tools: Value,
    pub content: String,
    pub version: i32,
    pub source: String,
    pub author_label: Option<String>,
    pub price_cents: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Tool {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub parameters: Value,
    pub permissions: Value,
    pub execution_location: String,
    pub code: String,
    pub checksum: String,
    pub version: i32,
    pub source: String,
    pub author_label: Option<String>,
    pub price_cents: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub role_description: String,
    pub system_prompt: String,
    pub tool_permissions: Value,
    pub wave: i32,
    pub is_builtin: bool,
    pub source: String,
    pub author_label: Option<String>,
    pub price_cents: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct CanvasPack {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub box_height_dp: i32,
    pub max_elements: i32,
    pub background_colour: Option<String>,
    pub version: i32,
    pub source: String,
    pub author_label: Option<String>,
    pub price_cents: i64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct CanvasElement {
    pub id: Uuid,
    pub pack_id: Uuid,
    pub sort_order: i32,
    pub element_type: String,
    pub name: String,
    pub text_content: Option<String>,
    pub script: Option<String>,
    pub media_path: Option<String>,
    pub media_mime: Option<String>,
    pub media_bytes: Option<i64>,
    pub checksum: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct ConfigDefault {
    pub key: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct ReviewToken {
    pub id: Uuid,
    pub label: String,
    pub token_hash: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct Submission {
    pub id: Uuid,
    pub content_type: String,
    pub slug: String,
    pub name: String,
    pub payload: Value,
    pub author_label: Option<String>,
    pub status: String,
    pub review_note: Option<String>,
    pub catalog_id: Option<Uuid>,
    pub submitted_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}