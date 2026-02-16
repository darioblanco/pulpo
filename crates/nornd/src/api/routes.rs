use std::sync::Arc;

use axum::{Router, routing::get};

use super::AppState;
use super::node;
use super::sessions;

pub fn build(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/node", get(node::get_info))
        .route(
            "/api/v1/sessions",
            get(sessions::list).post(sessions::create),
        )
        .route(
            "/api/v1/sessions/{id}",
            get(sessions::get).delete(sessions::kill),
        )
        .with_state(state)
}
