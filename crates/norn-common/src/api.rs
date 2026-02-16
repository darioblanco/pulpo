use serde::{Deserialize, Serialize};

use crate::session::Provider;

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: Option<String>,
    pub repo_path: String,
    pub provider: Option<Provider>,
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
