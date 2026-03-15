use rapina::prelude::*;
use rapina::database::Db;
use rapina::response::BoxBody;

use super::dto::{CreateUrlRequest, CreateUrlResponse, DeleteUrlResponse};
use super::error::UrlsError;
use super::service;

#[get("/", group = "/api/v1/shorten")]
#[errors(UrlsError)]
pub async fn list_urls(db: Db) -> Result<Json<Vec<crate::entity::urls::Model>>> {
    let items = service::list_all(db.conn()).await?;
    Ok(Json(items))
}

#[get("/:code", group = "/api/v1/shorten")]
#[public]
#[cache(ttl = 300)]
#[errors(UrlsError)]
pub async fn redirect(db: Db, code: Path<String>) -> Result<http::Response<BoxBody>> {
    let code = code.into_inner();
    let item = service::find_by_code(db.conn(), &code).await?;

    if service::is_expired(&item) {
        return Err(Error::new(410, "GONE", format!("URL '{}' has expired", code)));
    }

    service::increment_clicks(db.conn(), item.clone()).await?;

    let response = http::Response::builder()
        .status(http::StatusCode::FOUND)
        .header("Location", &item.long_url)
        .body(BoxBody::default())
        .unwrap();

    Ok(response)
}

#[public]
#[post("/", group = "/api/v1/shorten")]
#[errors(UrlsError)]
pub async fn create_url(db: Db, body: Validated<Json<CreateUrlRequest>>) -> Result<Json<CreateUrlResponse>> {
    let input = body.into_inner().into_inner();
    let result = service::create(db.conn(), input).await?;
    Ok(Json(CreateUrlResponse {
        short_code: result.short_code,
        long_url: result.long_url,
    }))
}

#[public]
#[delete("/:short_code", group = "/api/v1/shorten")]
#[errors(UrlsError)]
pub async fn delete_url(db: Db, code: Path<String>) -> Result<Json<DeleteUrlResponse>> {
    let code = code.into_inner();
    service::delete_by_code(db.conn(), &code).await?;
    Ok(Json(DeleteUrlResponse { deleted: code }))
}
