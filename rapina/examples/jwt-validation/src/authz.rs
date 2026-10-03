use rapina::jwt::JsonWebToken;
use rapina::prelude::*;

pub async fn authorize(token: &JsonWebToken) -> Result<()> {
    tracing::info!(sub = %token.sub, "authorizing request before it hits the handler");
    Err(Error::forbidden("forbidden from another module"))
}
