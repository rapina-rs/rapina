## Error handling

Each feature has a typed error enum:

```rust
pub enum TodoError {
    DbError(DbError),
}

impl IntoApiError for TodoError {
    fn into_api_error(self) -> Error {
        match self {
            TodoError::DbError(e) => e.into_api_error(),
        }
    }
}

impl DocumentedError for TodoError {
    fn error_variants() -> Vec<ErrorVariant> {
        vec![
            ErrorVariant { status: 404, code: "NOT_FOUND", description: "Todo not found" },
        ]
    }
}
```

All error responses include a `trace_id`:

```json
{
  "error": { "code": "NOT_FOUND", "message": "Todo 42 not found" },
  "trace_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

Use `Error::not_found()`, `Error::bad_request()`, `Error::unauthorized()` for quick errors.

Don't use `sqlx::query!`. Use the ORM layer.
