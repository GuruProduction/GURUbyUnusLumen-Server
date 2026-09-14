//! Community submission and review pipeline — the ONLY authenticated surface.
//!
//! flow: author POSTs a submission (JSON payload, no PII, optional pseudonym)
//!   -> it lands in the pending queue with staged media on disk (if any)
//!   -> reviewer approves: payload row is written to the catalog and the
//!      submission payload is nulled
//!   -> reviewer rejects: payload nulled in the same statement and staged
//!      media purged from disk immediately
//! Reviewers authenticate with hashed bearer tokens — the only token concept
//! on this server.

use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn review_router() -> Router<AppState> {
    Router::new()
        .route("/v1/submit/{content_type}", post(create_submission))
        .route("/v1/review/submissions", get(list_pending))
        .route("/v1/review/submissions/{id}", get(get_submission))
        .route("/v1/review/submissions/{id}/approve", post(approve))
        .route("/v1/review/submissions/{id}/reject", post(reject))
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}

async fn require_reviewer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Uuid, (StatusCode, String)> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".into()))?;

    let token_hash = hash_token(bearer);
    let token = state
        .reviews
        .verify_review_token(&token_hash)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal error".into(),
            )
        })?
        .ok_or((StatusCode::UNAUTHORIZED, "invalid token".into()))?;

    // fire-and-forget last-used touch; never blocks the decision
    let reviews = state.reviews.clone();
    let token_id = token.id;
    tokio::spawn(async move {
        let _ = reviews.touch_token(token_id).await;
    });

    Ok(token_id)
}

fn bad_request(msg: &str) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.into())
}

/// Validate the slug shape: lowercase ascii words joined by single dashes.
fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
}

// ---------------------------------------------------------------------------
// submission (public, unauthenticated, no PII)
// ---------------------------------------------------------------------------

const MAX_SUBMISSION_BYTES: usize = 2 * 1024 * 1024; // 2MB of JSON
const ALLOWED_TYPES: [&str; 4] = ["skill", "tool", "agent", "canvas_pack"];

async fn create_submission(
    State(state): State<AppState>,
    Path(content_type): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if !ALLOWED_TYPES.contains(&content_type.as_str()) {
        return bad_request("content_type must be one of skill, tool, agent, canvas_pack")
            .into_response();
    }
    if body.len() > MAX_SUBMISSION_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "submission exceeds 2MB; submit media packs by URL",
        )
            .into_response();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return bad_request(&format!("invalid JSON: {e}")).into_response();
        }
    };

    // Shape check: slug and name required at top level.
    let Some(slug) = payload.get("slug").and_then(|v| v.as_str()) else {
        return bad_request("payload missing 'slug'").into_response();
    };
    if !valid_slug(slug) {
        return bad_request("slug must be lowercase words joined by single dashes").into_response();
    }
    let Some(name) = payload.get("name").and_then(|v| v.as_str()) else {
        return bad_request("payload missing 'name'").into_response();
    };

    // PII guard: reject obvious email/phone fields anywhere in the payload.
    let payload_text = body.as_ref();
    if payload_text.windows(9).any(|w| w == b"\"email\":") || payload_text.windows(3).any(|w| w == b"@gmail" )
    {
        return bad_request(
            "submissions must not contain personal fields; keep them publishable-only",
        )
        .into_response();
    }

    let author_label = payload
        .get("author_label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let submission = state
        .reviews
        .create_submission(&content_type, slug, name, &payload, author_label)
        .await;

    match submission {
        Ok(s) => (
            StatusCode::CREATED,
            Json(json!({
                "id": s.id,
                "status": s.status,
                "content_type": s.content_type,
                "slug": s.slug,
                "submitted_at": s.submitted_at,
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "submission insert failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// review (authenticated)
// ---------------------------------------------------------------------------

async fn list_pending(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_reviewer(&state, &headers).await {
        return e.into_response();
    }
    match state.reviews.list_pending_submissions().await {
        Ok(rows) => {
            // Strip full payloads from the queue list; reviewers fetch the
            // individual submission for the payload.
            let summaries: Vec<Value> = rows
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id,
                        "content_type": s.content_type,
                        "slug": s.slug,
                        "name": s.name,
                        "author_label": s.author_label,
                        "submitted_at": s.submitted_at,
                    })
                })
                .collect();
            Json(summaries).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn get_submission(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_reviewer(&state, &headers).await {
        return e.into_response();
    }
    match state.reviews.get_submission(id).await {
        Ok(Some(s)) => Json(s).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such submission").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_reviewer(&state, &headers).await {
        return e.into_response();
    }

    let submission = match state.reviews.get_submission(id).await {
        Ok(Some(s)) if s.status == "pending" => s,
        Ok(_) => return (StatusCode::CONFLICT, "submission not pending").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    };

    let payload = &submission.payload;
    if payload.is_null() {
        return (StatusCode::CONFLICT, "submission payload missing").into_response();
    }

    // Promote the payload into the right catalog table.
    let catalog_id = Uuid::new_v4();
    let promote_result = match submission.content_type.as_str() {
        "skill" => promote_skill(&state, catalog_id, payload).await,
        "tool" => promote_tool(&state, catalog_id, payload).await,
        "agent" => promote_agent(&state, catalog_id, payload).await,
        "canvas_pack" => promote_canvas_pack(&state, catalog_id, payload).await,
        other => Err(format!("unknown content type {other}")),
    };

    if let Err(e) = promote_result {
        return bad_request(&e).into_response();
    }

    if let Err(e) = state.reviews.approve_submission(id, catalog_id, None).await {
        tracing::error!(error = %e, "approve failed after catalog insert");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
    }

    (
        StatusCode::OK,
        Json(json!({
            "id": id,
            "status": "approved",
            "catalog_id": catalog_id,
        })),
    )
        .into_response()
}

async fn reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_reviewer(&state, &headers).await {
        return e.into_response();
    }

    // The privacy guarantee executes here: payload nulled in the same UPDATE,
    // and any staged media on disk is removed immediately.
    if let Err(e) = state.reviews.reject_submission(id, None).await {
        tracing::error!(error = %e, "reject failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
    }
    purge_staged_media(&state, id).await;

    (
        StatusCode::OK,
        Json(json!({
            "id": id,
            "status": "rejected",
            "payload": "purged",
        })),
    )
        .into_response()
}

async fn purge_staged_media(state: &AppState, submission_id: Uuid) {
    let dir = state.content_dir.join("_staging").join(submission_id.to_string());
    if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, ?dir, "staged media purge failed");
        }
    }
}

// ---------------------------------------------------------------------------
// promotion: payload -> catalog row
// ---------------------------------------------------------------------------

fn payload_str<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("payload missing '{key}'"))
}

fn payload_json_or<'a>(payload: &'a Value, key: &str, fallback: &'a Value) -> &'a Value {
    payload.get(key).unwrap_or(fallback)
}

async fn promote_skill(state: &AppState, _id: Uuid, payload: &Value) -> Result<(), String> {
    let slug = payload_str(payload, "slug")?;
    let name = payload_str(payload, "name")?;
    let description = payload_str(payload, "description")?;
    let content = payload_str(payload, "content")?;
    let when_to_use = payload
        .get("when_to_use")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let empty: Value = serde_json::json!([]);
    let allowed_tools = payload_json_or(payload, "allowed_tools", &empty).clone();
    let author = payload
        .get("author_label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    state
        .content
        .insert_skill(
            Uuid::new_v4(),
            slug,
            name,
            description,
            &when_to_use,
            &allowed_tools,
            content,
            author,
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("skill promotion failed: {e}"))
}

async fn promote_tool(state: &AppState, _id: Uuid, payload: &Value) -> Result<(), String> {
    let slug = payload_str(payload, "slug")?;
    let name = payload_str(payload, "name")?;
    let description = payload_str(payload, "description")?;
    let category = payload
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("community")
        .to_string();
    let empty: Value = serde_json::json!([]);
    let parameters = payload_json_or(payload, "parameters", &empty).clone();
    let permissions = payload_json_or(payload, "permissions", &empty).clone();
    let execution_location = payload
        .get("execution_location")
        .and_then(|v| v.as_str())
        .unwrap_or("on-device");
    let code = payload
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let author = payload
        .get("author_label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    state
        .content
        .insert_tool(
            Uuid::new_v4(),
            slug,
            name,
            description,
            &category,
            &parameters,
            &permissions,
            execution_location,
            code,
            author,
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("tool promotion failed: {e}"))
}

async fn promote_agent(state: &AppState, _id: Uuid, payload: &Value) -> Result<(), String> {
    let slug = payload_str(payload, "slug")?;
    let name = payload_str(payload, "name")?;
    let role_description = payload
        .get("role_description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let system_prompt = payload_str(payload, "system_prompt")?;
    let empty: Value = serde_json::json!([]);
    let tool_permissions = payload_json_or(payload, "tool_permissions", &empty).clone();
    let author = payload
        .get("author_label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    state
        .content
        .insert_agent(
            Uuid::new_v4(),
            slug,
            name,
            role_description,
            system_prompt,
            &tool_permissions,
            author,
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("agent promotion failed: {e}"))
}

async fn promote_canvas_pack(state: &AppState, _id: Uuid, payload: &Value) -> Result<(), String> {
    let slug = payload_str(payload, "slug")?;
    let name = payload_str(payload, "name")?;
    let description = payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let box_height_dp = payload
        .get("box_height_dp")
        .and_then(|v| v.as_i64())
        .unwrap_or(220) as i32;
    let max_elements = payload
        .get("max_elements")
        .and_then(|v| v.as_i64())
        .unwrap_or(8) as i32;
    let background_colour = payload
        .get("background_colour")
        .and_then(|v| v.as_str());
    let author = payload
        .get("author_label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    // Elements are inline data at this stage (text elements, scripts). Media
    // arrives through staged uploads, referenced by name in media refs, and is
    // registered with a post-approval media-registration call.
    let mut elements: Vec<(i32, String, String, Option<String>, Option<String>)> = Vec::new();
    if let Some(arr) = payload.get("elements").and_then(|v| v.as_array()) {
        for (index, element) in arr.iter().enumerate() {
            let element_type = element
                .get("element_type")
                .and_then(|v| v.as_str())
                .ok_or("each element needs an element_type")?;
            let elem_name = element
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let text_content = element
                .get("text_content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let script = element
                .get("script")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Merge text_content and script into the tuple; media_path stays
            // empty for inline content.
            let text_for_db = match (text_content.clone(), script.clone()) {
                (Some(t), Some(s)) => Some(format!("{t}\n\n---script---\n{s}")),
                (Some(t), None) => Some(t),
                (None, Some(s)) => Some(s),
                (None, None) => None,
            };
            elements.push((
                index as i32,
                element_type.to_string(),
                elem_name.to_string(),
                text_for_db,
                None,
            ));
        }
    }

    state
        .content
        .insert_canvas_pack(
            Uuid::new_v4(),
            slug,
            name,
            description,
            box_height_dp,
            max_elements,
            background_colour,
            &elements,
            author,
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("canvas pack promotion failed: {e}"))
}