# url-shortner

A web application built with Rapina.

## Getting started

```bash
rapina dev
```

## Routes

- `GET /` — Hello world
- `GET /health` — Health check

# Problems so far

``` rapina add resource urls id:i64 short_code:string long_url:text created_at:datetime expires_at:datetime click_count:i64 ```
Created i64 instead of bigint
urls -> urlss

## after 
mod urlss;
mod entity;
mod migrations;
->
field 'created_at' is auto-generated. Use #[timestamps(none)] or #[timestamps(updated_at)] to declare it manually

added #[timestamps(none)] to entity

in #[post]
first declared short_code: Set(String::new()), inserted id into short_code, them updated short_code with base62 encoded id.
Maybe there is a more efficient way to do this.

## Routes added

- `GET /api/v1/shorten` — List all shortened URLs (requires auth)
- `POST /api/v1/shorten` — Create a new shortened URL (public)
- `GET /api/v1/shorten/:code` — Redirect to the original URL (public)
- `DELETE /api/v1/shorten/:code` — Delete a shortened URL 

## Implementation notes

### Database setup

Used `DatabaseConfig::new("sqlite://todos.db?mode=rwc")` to connect to a local SQLite file.
Migrations run automatically on startup via `.run_migrations::<migrations::Migrator>()`.

### Short code generation

On `POST /api/v1/shorten`, the short code is generated from the DB-assigned `id` using `base62::encode(id as u128 + 6767)`.
The `+ 6767` offset ensures codes start with a longer length, avoiding single-character codes for low IDs.
Two-step process: insert with empty `short_code`, then update after getting the generated `id`.

### Redirect handler

`GET /api/v1/shorten/:code` looks up the URL by `short_code`, increments `click_count`, then returns a `301 Moved Permanently` redirect to `long_url`.

The response body uses `rapina::response::BoxBody` (a type alias for `Full<Bytes>` from `http_body_util`).
This is an internal rapina type, not officially documented, found by inspecting the source.

```rust
use rapina::response::BoxBody;

let response = http::Response::builder()
    .status(http::StatusCode::MOVED_PERMANENTLY)
    .header("Location", &item.long_url)
    .body(BoxBody::default())
    .unwrap();
```

### ColumnTrait and QueryFilter

To filter by a column other than `id`, import `ColumnTrait` and `QueryFilter` from `rapina::sea_orm`:

```rust
use rapina::sea_orm::{ColumnTrait, QueryFilter};

Urls::find()
    .filter(crate::entity::urls::Column::ShortCode.eq(&code))
    .one(db.conn())
    .await
```
### Switch to 302 for click tracking