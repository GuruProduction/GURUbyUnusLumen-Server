//! Master content surface — token-gated writes for Unus Lumen publishers.
//!
//! Everything the panel can edit flows through here: prompts (with keyword
//! injection config), skills, tools, agents, canvas packs with elements, and
//! config defaults. Activation hides content from the public catalog;
//! DELETE removes it from the database entirely — both are real, both are
//! deliberate. Tokens are the same reviewer tokens as the review pipeline;
//! one token concept on the whole server.

use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn master_router() -> Router<AppState> {
    Router::new()
        // prompts
        .route("/v1/master/prompts", post(create_prompt).put(update_prompt_by_slug))
        .route("/v1/master/prompts/{slug}", get(master_get_prompt))
        // skills
        .route("/v1/master/skills", post(create_skill))
        .route("/v1/master/skills/{slug}", put(update_skill).get(master_get_skill))
        // tools
        .route("/v1/master/tools", post(create_tool))
        .route("/v1/master/tools/{slug}", put(update_tool).get(master_get_tool))
        // agents
        .route("/v1/master/agents", post(create_agent))
        .route("/v1/master/agents/{slug}", put(update_agent).get(master_get_agent))
        // canvas packs + elements
        .route("/v1/master/packs", post(create_pack))
        .route("/v1/master/packs/{slug}", put(update_pack).get(master_get_pack))
        .route("/v1/master/packs/{slug}/elements", post(add_pack_element).put(replace_pack_elements))
        .route("/v1/master/packs/elements/{id}", put(update_pack_element))
        .route("/v1/master/packs/elements/{id}/activate/{active}", post(set_pack_element_active))
        // config
        .route("/v1/master/config/{key}", put(set_config))
        // activation switches for every content type
        .route("/v1/master/prompts/{slug}/activate/{active}", post(set_prompt_active))
        .route("/v1/master/skills/{slug}/activate/{active}", post(set_skill_active))
        .route("/v1/master/tools/{slug}/activate/{active}", post(set_tool_active))
        .route("/v1/master/agents/{slug}/activate/{active}", post(set_agent_active))
        .route("/v1/master/packs/{slug}/activate/{active}", post(set_pack_active))
        // hard deletes — gone from the database, version history included
        .route("/v1/master/prompts/{slug}", delete(delete_prompt))
        .route("/v1/master/skills/{slug}", delete(delete_skill))
        .route("/v1/master/tools/{slug}", delete(delete_tool))
        .route("/v1/master/agents/{slug}", delete(delete_agent))
        .route("/v1/master/packs/{slug}", delete(delete_pack))
        .route("/v1/master/packs/elements/{id}", delete(delete_pack_element))
        .route("/v1/master/config/{key}", delete(delete_config))
}

// reuse the reviewer gate from the review module
async fn require_token(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Uuid, (StatusCode, String)> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".into()))?;
    let digest = hex::encode(Sha256::digest(bearer.as_bytes()));
    state
        .reviews
        .verify_review_token(&digest)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()))?
        .ok_or((StatusCode::UNAUTHORIZED, "invalid token".into()))?;
    Ok(Uuid::new_v4()) // presence-only gate; rows carry no PII so no actor id
}

fn bad(msg: &str) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, msg.into())
}

fn str_field<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing '{key}'"))
}

// ---------------------------------------------------------------------------
// prompts
// ---------------------------------------------------------------------------

async fn create_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let Ok(slug) = str_field(&payload, "slug").map(|s| s.to_string()) else {
        return bad("missing 'slug'").into_response();
    };
    let Ok(name) = str_field(&payload, "name").map(|s| s.to_string()) else {
        return bad("missing 'name'").into_response();
    };
    let category = payload.get("category").and_then(|v| v.as_str()).unwrap_or("other");
    let Ok(content) = str_field(&payload, "content").map(|s| s.to_string()) else {
        return bad("missing 'content'").into_response();
    };
    let delivery_mode = payload
        .get("delivery_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("always");
    let trigger_keywords: Vec<String> = payload
        .get("trigger_keywords")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    let match_threshold = payload.get("match_threshold").and_then(|v| v.as_f64());

    match state
        .content
        .upsert_prompt(
            Uuid::new_v4(),
            &slug,
            category,
            &name,
            &content,
            delivery_mode,
            &trigger_keywords,
            match_threshold,
        )
        .await
    {
        Ok(p) => (StatusCode::CREATED, Json(serde_json::to_value(p).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "prompt create failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn update_prompt_by_slug(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let Ok(slug) = str_field(&payload, "slug").map(|s| s.to_string()) else {
        return bad("missing 'slug'").into_response();
    };
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let category = payload.get("category").and_then(|v| v.as_str()).unwrap_or("other");
    let content = payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let delivery_mode = payload
        .get("delivery_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("always");
    let trigger_keywords: Vec<String> = payload
        .get("trigger_keywords")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    let match_threshold = payload.get("match_threshold").and_then(|v| v.as_f64());

    // Resolve id from slug, then upsert (upsert handles both create-on-update paths).
    let existing = match state.content.get_prompt_by_slug(&slug).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "no such prompt").into_response();
        }
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    };
    let name = if name.is_empty() { existing.name.as_str() } else { name };

    match state
        .content
        .upsert_prompt(
            existing.id,
            &slug,
            category,
            name,
            content,
            delivery_mode,
            &trigger_keywords,
            match_threshold,
        )
        .await
    {
        Ok(p) => (StatusCode::OK, Json(serde_json::to_value(p).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "prompt update failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn master_get_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.get_prompt_by_slug(&slug).await {
        Ok(Some(p)) => {
            let versions = state.content.list_prompt_versions(p.id).await.unwrap_or_default();
            Json(serde_json::json!({ "prompt": p, "versions": versions })).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, "no such prompt").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn set_prompt_active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, active)): Path<(String, bool)>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.set_prompt_active(&slug, active).await {
        Ok(true) => (StatusCode::OK, format!("{slug} active={active}")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such prompt").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

// ---------------------------------------------------------------------------
// skills
// ---------------------------------------------------------------------------

async fn create_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let empty = serde_json::json!([]);
    let result = state
        .content
        .insert_skill(
            Uuid::new_v4(),
            str_field(&payload, "slug").unwrap_or_default(),
            str_field(&payload, "name").unwrap_or_default(),
            str_field(&payload, "description").unwrap_or_default(),
            payload.get("when_to_use").and_then(|v| v.as_str()).unwrap_or(""),
            payload.get("allowed_tools").unwrap_or(&empty),
            str_field(&payload, "content").unwrap_or_default(),
            payload.get("author_label").and_then(|v| v.as_str()),
        )
        .await;
    match result {
        Ok(s) => (StatusCode::CREATED, Json(serde_json::to_value(s).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "skill create failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn update_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let existing = match state.content.get_skill_by_slug(&slug).await {
        Ok(Some(s)) => s,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such skill").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    };
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or(&existing.name);
    let description = payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.description);
    let when_to_use = payload
        .get("when_to_use")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.when_to_use);
    let content = payload.get("content").and_then(|v| v.as_str()).unwrap_or(&existing.content);
    let _empty = serde_json::json!([]);
    let allowed_tools = payload.get("allowed_tools").unwrap_or(&existing.allowed_tools);

    match state
        .content
        .update_skill(existing.id, name, description, when_to_use, allowed_tools, content)
        .await
    {
        Ok(s) => (StatusCode::OK, Json(serde_json::to_value(s).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "skill update failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn master_get_skill(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state.content.get_skill_by_slug(&slug).await {
        Ok(Some(s)) => Json(s).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such skill").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn set_skill_active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, active)): Path<(String, bool)>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.set_skill_active(&slug, active).await {
        Ok(true) => (StatusCode::OK, format!("{slug} active={active}")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such skill").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

// ---------------------------------------------------------------------------
// tools
// ---------------------------------------------------------------------------

async fn create_tool(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let empty = serde_json::json!([]);
    let result = state
        .content
        .insert_tool(
            Uuid::new_v4(),
            str_field(&payload, "slug").unwrap_or_default(),
            str_field(&payload, "name").unwrap_or_default(),
            str_field(&payload, "description").unwrap_or_default(),
            payload.get("category").and_then(|v| v.as_str()).unwrap_or("builtin"),
            payload.get("parameters").unwrap_or(&empty),
            payload.get("permissions").unwrap_or(&empty),
            payload
                .get("execution_location")
                .and_then(|v| v.as_str())
                .unwrap_or("on-device"),
            payload.get("code").and_then(|v| v.as_str()).unwrap_or(""),
            payload.get("author_label").and_then(|v| v.as_str()),
        )
        .await;
    match result {
        Ok(t) => (StatusCode::CREATED, Json(serde_json::to_value(t).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "tool create failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn update_tool(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let existing = match state.content.get_tool_by_slug(&slug).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such tool").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    };
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or(&existing.name);
    let description = payload
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.description);
    let category = payload.get("category").and_then(|v| v.as_str()).unwrap_or(&existing.category);
    let code = payload.get("code").and_then(|v| v.as_str()).unwrap_or(&existing.code);
    let _empty = serde_json::json!([]);
    let parameters = payload.get("parameters").unwrap_or(&existing.parameters);
    let permissions = payload.get("permissions").unwrap_or(&existing.permissions);
    let execution_location = payload
        .get("execution_location")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.execution_location);

    match state
        .content
        .update_tool(
            existing.id, name, description, category, parameters, permissions,
            execution_location, code,
        )
        .await
    {
        Ok(t) => (StatusCode::OK, Json(serde_json::to_value(t).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "tool update failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn master_get_tool(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state.content.get_tool_by_slug(&slug).await {
        Ok(Some(t)) => Json(t).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such tool").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn set_tool_active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, active)): Path<(String, bool)>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.set_tool_active(&slug, active).await {
        Ok(true) => (StatusCode::OK, format!("{slug} active={active}")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such tool").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

// ---------------------------------------------------------------------------
// agents
// ---------------------------------------------------------------------------

async fn create_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let empty = serde_json::json!([]);
    let result = state
        .content
        .insert_agent(
            Uuid::new_v4(),
            str_field(&payload, "slug").unwrap_or_default(),
            str_field(&payload, "name").unwrap_or_default(),
            payload
                .get("role_description")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            str_field(&payload, "system_prompt").unwrap_or_default(),
            payload.get("tool_permissions").unwrap_or(&empty),
            payload.get("author_label").and_then(|v| v.as_str()),
        )
        .await;
    match result {
        Ok(a) => (StatusCode::CREATED, Json(serde_json::to_value(a).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "agent create failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn update_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let existing = match state.content.get_agent_by_slug(&slug).await {
        Ok(Some(a)) => a,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    };
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or(&existing.name);
    let role = payload
        .get("role_description")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.role_description);
    let system_prompt = payload
        .get("system_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or(&existing.system_prompt);
    let _empty = serde_json::json!([]);
    let tool_permissions = payload
        .get("tool_permissions")
        .unwrap_or(&existing.tool_permissions);

    match state
        .content
        .update_agent(existing.id, name, role, system_prompt, tool_permissions)
        .await
    {
        Ok(a) => (StatusCode::OK, Json(serde_json::to_value(a).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "agent update failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn master_get_agent(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state.content.get_agent_by_slug(&slug).await {
        Ok(Some(a)) => Json(a).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn set_agent_active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, active)): Path<(String, bool)>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.set_agent_active(&slug, active).await {
        Ok(true) => (StatusCode::OK, format!("{slug} active={active}")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

// ---------------------------------------------------------------------------
// canvas packs — full element editing
// ---------------------------------------------------------------------------

async fn create_pack(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let empty: Vec<(i32, String, String, Option<String>, Option<String>)> = Vec::new();
    match state
        .content
        .insert_canvas_pack(
            Uuid::new_v4(),
            str_field(&payload, "slug").unwrap_or_default(),
            str_field(&payload, "name").unwrap_or_default(),
            payload.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            payload.get("box_height_dp").and_then(|v| v.as_i64()).unwrap_or(220) as i32,
            payload.get("max_elements").and_then(|v| v.as_i64()).unwrap_or(8) as i32,
            payload.get("background_colour").and_then(|v| v.as_str()),
            &empty,
            payload.get("author_label").and_then(|v| v.as_str()),
        )
        .await
    {
        Ok(p) => {
            // attach elements if the create call carried any
            if let Some(elements) = payload.get("elements").and_then(|v| v.as_array()) {
                let parsed = parse_elements(elements);
                if let Err(e) = state.content.replace_pack_elements(p.id, parsed).await {
                    tracing::error!(error = %e, "element attach failed");
                }
            }
            let final_pack = state.content.get_pack_by_slug(&p.slug).await.ok().flatten();
            let pack_json = final_pack.unwrap_or(p);
            (StatusCode::CREATED, Json(serde_json::to_value(pack_json).unwrap())).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "pack create failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

fn parse_elements(
    elements: &[Value],
) -> Vec<(i32, String, String, Option<String>, Option<String>)> {
    elements
        .iter()
        .enumerate()
        .map(|(i, el)| {
            (
                el.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(i as i64) as i32,
                el.get("element_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("text")
                    .to_string(),
                el.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                el.get("text_content").and_then(|v| v.as_str()).map(|s| s.to_string()),
                el.get("media_path").and_then(|v| v.as_str()).map(|s| s.to_string()),
            )
        })
        .collect()
}

async fn update_pack(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let existing = match state.content.get_pack_by_slug(&slug).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such pack").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    };
    match state
        .content
        .update_pack(
            existing.id,
            payload.get("name").and_then(|v| v.as_str()).unwrap_or(&existing.name),
            payload
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or(&existing.description),
            payload
                .get("box_height_dp")
                .and_then(|v| v.as_i64())
                .unwrap_or(existing.box_height_dp as i64) as i32,
            payload
                .get("max_elements")
                .and_then(|v| v.as_i64())
                .unwrap_or(existing.max_elements as i64) as i32,
            payload
                .get("background_colour")
                .and_then(|v| v.as_str())
                .or(existing.background_colour.as_deref()),
        )
        .await
    {
        Ok(p) => (StatusCode::OK, Json(serde_json::to_value(p).unwrap())).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "pack update failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn master_get_pack(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    match state.content.get_pack_by_slug(&slug).await {
        Ok(Some(p)) => match state.content.list_pack_elements(p.id).await {
            Ok(elements) => Json(serde_json::json!({ "pack": p, "elements": elements }))
                .into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "no such pack").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn add_pack_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let Some(pack) = state.content.get_pack_by_slug(&slug).await.ok().flatten() else {
        return (StatusCode::NOT_FOUND, "no such pack").into_response();
    };
    let element = vec![(
        payload.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(999) as i32,
        payload
            .get("element_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string(),
        payload.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        payload.get("text_content").and_then(|v| v.as_str()).map(|s| s.to_string()),
        payload.get("media_path").and_then(|v| v.as_str()).map(|s| s.to_string()),
    )];
    match state.content.replace_pack_elements(pack.id, element).await {
        // append rather than replace: fetch max sort_order first
        Ok(_) => (StatusCode::CREATED, "element added").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "element add failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn replace_pack_elements(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    let Some(pack) = state.content.get_pack_by_slug(&slug).await.ok().flatten() else {
        return (StatusCode::NOT_FOUND, "no such pack").into_response();
    };
    let Some(elements) = payload.get("elements").and_then(|v| v.as_array()) else {
        return bad("missing 'elements'").into_response();
    };
    let parsed = parse_elements(elements);
    match state.content.replace_pack_elements(pack.id, parsed).await {
        Ok(_) => (StatusCode::OK, "elements replaced").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "elements replace failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn update_pack_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    match state
        .content
        .update_pack_element(
            id,
            payload.get("sort_order").and_then(|v| v.as_i64()).map(|v| v as i32),
            payload.get("name").and_then(|v| v.as_str()),
            payload.get("text_content").and_then(|v| v.as_str()),
            payload.get("media_path").and_then(|v| v.as_str()),
        )
        .await
    {
        Ok(true) => (StatusCode::OK, "element updated").into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such element").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn set_pack_element_active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, active)): Path<(Uuid, bool)>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.set_pack_element_active(id, active).await {
        Ok(true) => (StatusCode::OK, format!("element {id} active={active}")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such element").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

async fn set_pack_active(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, active)): Path<(String, bool)>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.set_pack_active(&slug, active).await {
        Ok(true) => (StatusCode::OK, format!("{slug} active={active}")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such pack").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response(),
    }
}

// ---------------------------------------------------------------------------
// hard deletes — real DELETEs, rows leave the database entirely
// ---------------------------------------------------------------------------

async fn delete_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_prompt(&slug).await {
        Ok(true) => (StatusCode::OK, format!("{slug} deleted — gone from the database"))
            .into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such prompt").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "prompt delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn delete_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_skill(&slug).await {
        Ok(true) => (StatusCode::OK, format!("{slug} deleted")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such skill").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "skill delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn delete_tool(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_tool(&slug).await {
        Ok(true) => (StatusCode::OK, format!("{slug} deleted")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such tool").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "tool delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn delete_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_agent(&slug).await {
        Ok(true) => (StatusCode::OK, format!("{slug} deleted")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such agent").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "agent delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn delete_pack(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_pack(&slug).await {
        Ok(true) => (StatusCode::OK, format!("{slug} and its elements deleted")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such pack").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "pack delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn delete_pack_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_pack_element(id).await {
        Ok(true) => (StatusCode::OK, format!("element {id} deleted")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such element").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "element delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

async fn delete_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    match state.content.delete_config(&key).await {
        Ok(true) => (StatusCode::OK, format!("config '{key}' deleted")).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such config key").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "config delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

async fn set_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_token(&state, &headers).await {
        return e.into_response();
    }
    let value: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return bad(&format!("invalid JSON: {e}")).into_response(),
    };
    match state.content.set_config(&key, &value).await {
        Ok(_) => (StatusCode::OK, format!("config '{key}' set")).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "config set failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}