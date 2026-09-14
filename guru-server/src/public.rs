//! Public read API — the publisher surface.
//!
//! Every route here is public, no auth, by design. The server publishes
//! content; it collects nothing. Apps cheap-check /v1/manifest with
//! If-None-Match, then pull only what changed.

use crate::etag::{if_none_match_304, Etagged};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use guru_db::repositories::ContentRepository;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Serialize)]
struct ManifestEntry {
    slug: String,
    version: i32,
}

#[derive(Serialize)]
struct Manifest {
    generated_at: String,
    prompts: Vec<ManifestEntry>,
    skills: Vec<ManifestEntry>,
    tools: Vec<ManifestEntry>,
    agents: Vec<ManifestEntry>,
    canvas_packs: Vec<ManifestEntry>,
}

pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/prompts", get(list_prompts))
        .route("/v1/prompts/{slug}", get(get_prompt))
        .route("/v1/skills", get(list_skills))
        .route("/v1/skills/{slug}", get(get_skill))
        .route("/v1/tools", get(list_tools))
        .route("/v1/tools/{slug}", get(get_tool))
        .route("/v1/agents", get(list_agents))
        .route("/v1/agents/{slug}", get(get_agent))
        .route("/v1/canvas-packs", get(list_packs))
        .route("/v1/canvas-packs/{slug}", get(get_pack))
        .route("/v1/canvas-packs/{slug}/elements", get(list_pack_elements))
        .route("/v1/canvas-media/{element_id}", get(get_pack_media))
        .route("/v1/config", get(get_config))
        .route("/v1/builtin-masks", get(get_builtin_masks))
}

async fn manifest(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let etag = format!(
        "\"guru-manifest-{}\"",
        state
            .content
            .content_state_hash()
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "manifest hash failed");
                "unavailable".into()
            })
    );

    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }

    let prompts = slug_versions(&state.content, "prompt_sections").await;
    let skills = slug_versions(&state.content, "skills").await;
    let tools = slug_versions(&state.content, "tools").await;
    let agents = slug_versions(&state.content, "agents").await;
    let canvas_packs = slug_versions(&state.content, "canvas_packs").await;

    let manifest = Manifest {
        generated_at: chrono::Utc::now().to_rfc3339(),
        prompts,
        skills,
        tools,
        agents,
        canvas_packs,
    };

    Etagged::new(etag, manifest).into_response()
}

async fn slug_versions(content: &ContentRepository, table: &'static str) -> Vec<ManifestEntry> {
    content
        .slug_versions(table)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(slug, version)| ManifestEntry { slug, version })
        .collect()
}

async fn list_prompts(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Ok(prompts) = state.content.list_active_prompts().await else {
        return internal_error().into_response();
    };
    let etag = list_etag("prompts", &prompts.iter().map(|p| (p.slug.clone(), p.version, p.updated_at)).collect::<Vec<_>>());
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, prompts).into_response()
}

async fn get_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Ok(Some(prompt)) = state.content.get_prompt_by_slug(&slug).await else {
        return not_found(slug).into_response();
    };
    let etag = format!("\"prompt-{}-v{}\"", prompt.slug, prompt.version);
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, prompt).into_response()
}

async fn list_skills(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Ok(skills) = state.content.list_active_skills().await else {
        return internal_error().into_response();
    };
    let etag = list_etag("skills", &skills.iter().map(|s| (s.slug.clone(), s.version, s.updated_at)).collect::<Vec<_>>());
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, skills).into_response()
}

async fn get_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Ok(Some(skill)) = state.content.get_skill_by_slug(&slug).await else {
        return not_found(slug).into_response();
    };
    let etag = format!("\"skill-{}-v{}\"", skill.slug, skill.version);
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, skill).into_response()
}

async fn list_tools(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Ok(tools) = state.content.list_active_tools().await else {
        return internal_error().into_response();
    };
    let etag = list_etag("tools", &tools.iter().map(|t| (t.slug.clone(), t.version, t.updated_at)).collect::<Vec<_>>());
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, tools).into_response()
}

async fn get_tool(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Ok(Some(tool)) = state.content.get_tool_by_slug(&slug).await else {
        return not_found(slug).into_response();
    };
    let etag = format!("\"tool-{}-v{}\"", tool.slug, tool.version);
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, tool).into_response()
}

async fn list_agents(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Ok(agents) = state.content.list_active_agents().await else {
        return internal_error().into_response();
    };
    let etag = list_etag("agents", &agents.iter().map(|a| (a.slug.clone(), 1, a.updated_at)).collect::<Vec<_>>());
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, agents).into_response()
}

async fn get_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Ok(Some(agent)) = state.content.get_agent_by_slug(&slug).await else {
        return not_found(slug).into_response();
    };
    let etag = format!("\"agent-{}\"", agent.slug);
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, agent).into_response()
}

async fn list_packs(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Ok(packs) = state.content.list_active_packs().await else {
        return internal_error().into_response();
    };
    let etag = list_etag("canvas-packs", &packs.iter().map(|p| (p.slug.clone(), p.version, p.updated_at)).collect::<Vec<_>>());
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, packs).into_response()
}

async fn get_pack(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Ok(Some(pack)) = state.content.get_pack_by_slug(&slug).await else {
        return not_found(slug).into_response();
    };
    let Ok(elements) = state.content.list_pack_elements(pack.id).await else {
        return internal_error().into_response();
    };
    let etag = format!("\"pack-{}-v{}\"", pack.slug, pack.version);
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    Etagged::new(etag, json!({
        "pack": pack,
        "elements": elements,
    }))
    .into_response()
}

async fn list_pack_elements(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> impl IntoResponse {
    let Ok(Some(pack)) = state.content.get_pack_by_slug(&slug).await else {
        return not_found(slug).into_response();
    };
    match state.content.list_pack_elements(pack.id).await {
        Ok(elements) => Json(elements).into_response(),
        Err(_) => internal_error().into_response(),
    }
}

async fn get_pack_media(
    State(state): State<AppState>,
    Path(element_id): Path<Uuid>,
) -> impl IntoResponse {
    let Ok(Some(element)) = state.content.get_element_by_id(element_id).await else {
        return not_found(element_id.to_string()).into_response();
    };
    let Some(media_path) = element.media_path else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "element has no media",
        )
            .into_response();
    };

    // Defense: media_path is relative and must never escape content_dir.
    let path = state.content_dir.join(&media_path);
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| state.content_dir.clone());
    if !canonical.starts_with(&state.content_dir) {
        return (StatusCode::FORBIDDEN, "path escapes content dir").into_response();
    }

    let Ok(bytes) = tokio::fs::read(&canonical).await else {
        return not_found(media_path).into_response();
    };

    let mime = element
        .media_mime
        .clone()
        .unwrap_or_else(|| "application/octet-stream".into());

    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&mime) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&format!("\"media-{}\"", element.id)) {
        headers.insert(header::ETAG, value);
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    (StatusCode::OK, headers, bytes).into_response()
}

async fn get_config(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Ok(config) = state.content.list_config().await else {
        return internal_error().into_response();
    };
    let etag = list_etag("config", &config.iter().map(|c| (c.key.clone(), 0, c.updated_at)).collect::<Vec<_>>());
    if let Some(response) = if_none_match_304(&headers, &etag) {
        return response;
    }
    let map: serde_json::Map<String, Value> = config
        .into_iter()
        .map(|c| (c.key, c.value))
        .collect();
    Etagged::new(etag, Value::Object(map)).into_response()
}

/// The 21 shipped mask templates as pure JSON — served from the binary so a
/// fresh install has agents before it ever syncs the community catalog.
async fn get_builtin_masks() -> impl IntoResponse {
    Json(guru_content::built_in_masks()).into_response()
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn list_etag(
    label: &str,
    rows: &[(String, i32, chrono::DateTime<chrono::Utc>)],
) -> String {
    let mut material: Vec<String> = rows
        .iter()
        .map(|(slug, version, updated)| format!("{slug}:{version}:{updated}"))
        .collect();
    material.sort();
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for part in &material {
            hasher.update(part.as_bytes());
        }
        hex::encode(hasher.finalize())
    };
    format!("\"{label}-{digest}\"")
}

fn not_found(what: String) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("not found: {what}"))
}

fn internal_error() -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
}