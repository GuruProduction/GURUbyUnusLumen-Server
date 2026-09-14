//! ETag helpers. Every public read endpoint emits an ETag derived from the
//! content state hash (or per-type hashes), so apps can cheap-check freshness
//! with If-None-Match and get 304s instead of re-downloading payloads.

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// A serializable value with a strong ETag that can short-circuit to 304.
pub struct Etagged<T> {
    pub etag: String,
    pub value: T,
}

impl<T: serde::Serialize> Etagged<T> {
    pub fn new(etag: String, value: T) -> Self {
        Self { etag, value }
    }
}

impl<T: serde::Serialize> IntoResponse for Etagged<T> {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ETAG,
            HeaderValue::from_str(&self.etag).expect("etag is always a valid header value"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=300"),
        );
        (
            StatusCode::OK,
            headers,
            axum::Json(self.value),
        )
            .into_response()
    }
}

/// Check the request's If-None-Match against an ETag. When it matches, return
/// the 304 short-circuit response with the same ETag.
pub fn if_none_match_304(headers: &HeaderMap, etag: &str) -> Option<Response> {
    let inm = headers.get(header::IF_NONE_MATCH)?;
    let values = inm.to_str().ok()?;
    // Weak comparison: ignore the W/ prefix clients may send.
    let normalized: Vec<String> = values
        .split(',')
        .map(|v| v.trim().trim_start_matches("W/").to_string())
        .collect();
    if normalized.iter().any(|v| v == etag || v == "*") {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ETAG,
            HeaderValue::from_str(etag).expect("etag is always a valid header value"),
        );
        return Some((StatusCode::NOT_MODIFIED, headers).into_response());
    }
    None
}