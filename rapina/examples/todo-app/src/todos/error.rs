use rapina::database::DbError;
use rapina::prelude::*;

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
            ErrorVariant {
                status: 404,
                code: "NOT_FOUND",
                description: "Todo not found",
            },
            ErrorVariant {
                status: 500,
                code: "DATABASE_ERROR",
                description: "Database operation failed",
            },
        ]
    }
}

impl From<DbError> for TodoError {
    fn from(e: DbError) -> Self {
        TodoError::DbError(e)
    }
}
