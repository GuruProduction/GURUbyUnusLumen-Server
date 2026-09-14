//! Shared application state for the GURU open server.
//!
//! Cheaply cloneable, passed to every axum handler through State. Holds the
//! content repository, the review repository, and the content directory path
//! where pack media lives on disk.

use guru_db::repositories::{ContentRepository, ReviewRepository};
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    pub content: ContentRepository,
    pub reviews: ReviewRepository,
    pub content_dir: PathBuf,
}