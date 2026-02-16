pub mod node;
pub mod routes;
pub mod sessions;

use std::sync::Arc;

use axum::Router;

use crate::config::Config;
use crate::store::Store;

pub struct AppState {
    pub config: Config,
    #[allow(dead_code)]
    pub store: Store,
}

impl AppState {
    pub fn new(config: Config, store: Store) -> Arc<Self> {
        Arc::new(Self { config, store })
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    routes::build(state)
}
