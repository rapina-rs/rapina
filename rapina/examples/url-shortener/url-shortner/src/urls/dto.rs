use rapina::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema, validator::Validate)]
pub struct CreateUrlRequest {
    #[validate(url)]
    pub long_url: String,
    pub expires_at: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct CreateUrlResponse {
    pub short_code: String,
    pub long_url: String,
}

#[derive(Serialize, JsonSchema)]
pub struct DeleteUrlResponse {
    pub deleted: String,
}

