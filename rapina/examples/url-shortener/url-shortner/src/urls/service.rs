use rapina::database::DbError;
use rapina::prelude::*;
use rapina::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};

use crate::entity::Urls;
use crate::entity::urls::{ActiveModel, Model};

use super::dto::CreateUrlRequest;

pub async fn list_all(conn: &rapina::sea_orm::DatabaseConnection) -> Result<Vec<Model>> {
    Urls::find().all(conn).await.map_err(|e| DbError(e).into_api_error())
}

pub async fn find_by_code(conn: &rapina::sea_orm::DatabaseConnection, code: &str) -> Result<Model> {
    Urls::find()
        .filter(crate::entity::urls::Column::ShortCode.eq(code))
        .one(conn)
        .await
        .map_err(|e| DbError(e).into_api_error())?
        .ok_or_else(|| Error::not_found(format!("URL with code '{}' not found", code)))
}

pub fn is_expired(item: &Model) -> bool {
    let now = rapina::sea_orm::prelude::DateTimeUtc::from(std::time::SystemTime::now());
    item.expires_at < now
}

pub async fn increment_clicks(conn: &rapina::sea_orm::DatabaseConnection, item: Model) -> Result<()> {
    let mut active: ActiveModel = item.clone().into_active_model();
    active.click_count = Set(item.click_count + 1);
    active.update(conn).await.map_err(|e| DbError(e).into_api_error())?;
    Ok(())
}

pub async fn create(conn: &rapina::sea_orm::DatabaseConnection, input: CreateUrlRequest) -> Result<Model> {
    let item = ActiveModel {
        short_code: Set(String::new()),
        long_url: Set(input.long_url),
        created_at: Set(rapina::sea_orm::prelude::DateTimeUtc::from(
            std::time::SystemTime::now() + std::time::Duration::from_secs(9 * 3600),
        )),
        expires_at: Set(input
            .expires_at
            .and_then(|s| s.parse::<rapina::sea_orm::prelude::DateTimeUtc>().ok())
            .unwrap_or_else(|| {
                rapina::sea_orm::prelude::DateTimeUtc::from(
                    std::time::SystemTime::now() + std::time::Duration::from_secs(24 * 365 * 3600),
                )
            })),
        click_count: Set(0),
        ..Default::default()
    };
    let inserted = item.insert(conn).await.map_err(|e| DbError(e).into_api_error())?;

    let mut active: ActiveModel = inserted.into_active_model();
    active.short_code = Set(base62::encode(active.id.clone().unwrap() as u128 + 6767u128));
    let result = active.update(conn).await.map_err(|e| DbError(e).into_api_error())?;
    Ok(result)
}

pub async fn delete_by_code(conn: &rapina::sea_orm::DatabaseConnection, code: &str) -> Result<()> {
    let result = Urls::delete_many()
        .filter(crate::entity::urls::Column::ShortCode.eq(code))
        .exec(conn)
        .await
        .map_err(|e| DbError(e).into_api_error())?;

    if result.rows_affected == 0 {
        return Err(Error::not_found(format!("URL '{}' not found", code)));
    }
    Ok(())
}
